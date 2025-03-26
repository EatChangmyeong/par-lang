// why not rename this file to parser.rs?

use super::{
    language::{
        Apply, ApplyBranch, ApplyBranches, Command, CommandBranch, CommandBranches, Construct,
        ConstructBranch, ConstructBranches, Expression, Pattern, Process,
    },
    lexer::{lex, Input, Token, TokenKind},
    types::Type,
};
use core::{fmt::Display, str::FromStr};
use indexmap::IndexMap;
use miette::{SourceOffset, SourceSpan};
use winnow::{
    combinator::{
        alt, cut_err, delimited, empty, not, opt, peek, preceded, repeat, separated, terminated,
        trace,
    },
    error::{
        AddContext, ContextError, ErrMode, ModalError, ParserError, StrContext, StrContextValue,
    },
    stream::{Accumulate, Compare, Stream, StreamIsPartial},
    token::any,
    Parser,
};
use winnow::token::literal;
use crate::par::language::{Declaration, Definition, Name, Program, TypeDef};

impl From<&Token<'_>> for Name {
    fn from(token: &Token) -> Self {
        Self {
            span: token.span,
            string: token.raw.to_owned()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MyError<C = StrContext> {
    context: Vec<(usize, ContextError<C>)>,
}
// impl<C> MyError<C> {
//     fn eof_offset(&self) -> usize {
//         self.context.iter().map(|x| x.0).min().unwrap_or(usize::MAX)
//     }
// }
pub type Error_ = MyError;
pub type Error = ErrMode<Error_>;
impl<I: Stream, C: core::fmt::Debug> ParserError<I> for MyError<C> {
    type Inner = Self;

    fn from_input(input: &I) -> Self {
        Self {
            context: vec![(input.eof_offset(), ContextError::from_input(input))],
        }
    }
    fn into_inner(self) -> winnow::Result<Self::Inner, Self> {
        Ok(self)
    }
    fn append(self, _input: &I, _token_start: &<I as Stream>::Checkpoint) -> Self {
        self
    }
    fn or(mut self, other: Self) -> Self {
        self.context.extend(other.context);
        self
    }
}
impl<I: Stream, C> AddContext<I, C> for MyError<C> {
    fn add_context(
        mut self,
        input: &I,
        token_start: &<I as Stream>::Checkpoint,
        context: C,
    ) -> Self {
        let new_context = |context| {
            (
                input.eof_offset(),
                ContextError::new().add_context(input, token_start, context),
            )
        };
        if self.context.is_empty() {
            self.context.push(new_context(context));
            return self;
        }
        let last = self.context.pop().unwrap();
        if last.0 != input.eof_offset() {
            self.context.push(new_context(context));
            return self;
        }
        let last = (
            last.0.min(input.eof_offset()),
            last.1.add_context(input, token_start, context),
        );
        self.context.push(last);
        self
    }
}

pub type Result<O, E = MyError> = core::result::Result<O, ErrMode<E>>;

/// Token with additional context of expecting the `token` value
fn t<'i, E>(kind: TokenKind) -> impl Parser<Input<'i>, &'i Token<'i>, E>
where
    E: AddContext<Input<'i>, StrContext> + ParserError<Input<'i>>,
{
    literal(kind)
        .context(StrContext::Expected(StrContextValue::StringLiteral(kind.expected())))
        .map(|t: &[Token]| &t[0])
}

/// Like `t` for but for `n` tokens.
macro_rules! tn {
    ($s:literal: $($t:expr),+) => {
        ($($t),+).context(StrContext::Expected(StrContextValue::Description($s)))
    };
}

fn list<'i, P, O>(item: P) -> impl Parser<Input<'i>, Vec<O>, Error> + use<'i, P, O>
where
    P: Parser<Input<'i>, O, Error>,
    Vec<O>: Accumulate<O>,
{
    terminated(separated(1.., item, t(TokenKind::Comma)), opt(t(TokenKind::Comma)))
}

fn commit_after<Input, Prefix, Output, Error, PrefixParser, ParseNext>(
    prefix: PrefixParser,
    parser: ParseNext,
) -> impl Parser<Input, (Prefix, Output), Error>
where
    Input: Stream,
    Error: ParserError<Input> + ModalError,
    PrefixParser: Parser<Input, Prefix, Error>,
    ParseNext: Parser<Input, Output, Error>,
{
    trace("commit_after", (
        prefix,
        cut_err(parser),
    ))
}

/// Like `commit_after` but the prefix is optional and only cuts if the prefix is `Some`.
/// Also returns the prefix.
fn opt_commit_after<Input, Prefix, Output, Error, PrefixParser, ParseNext>(
    prefix: PrefixParser,
    mut parser: ParseNext,
) -> impl Parser<Input, (Option<Prefix>, Output), Error>
where
    Input: Stream,
    Error: ParserError<Input> + ModalError,
    PrefixParser: Parser<Input, Prefix, Error>,
    ParseNext: Parser<Input, Output, Error>,
{
    let mut prefix = opt(prefix);
    trace("opt_commit_after", move |input: &mut Input| {
        let prefix = prefix.parse_next(input)?;
        if prefix.is_some() {
            let n = cut_err(parser.by_ref()).parse_next(input)?;
            Ok((prefix, n))
        } else {
            let n = parser.parse_next(input)?;
            Ok((prefix, n))
        }
    })
}

pub fn comment<'s, E>() -> impl Parser<&'s str, &'s str, E>
where
    E: ParserError<&'s str>,
{
    // below should be a valid block comment
    /* /* */ */
    // So have to consider nested comments
    let comment_block_rest = move |input: &mut &'s str| -> core::result::Result<(), E> {
        let mut nesting = 0;
        loop {
            let next_2 = match input.len() {
                0 => break Ok(()),
                1 => break Err(ParserError::from_input(input)),
                _ => &input.as_bytes()[..2],
            };
            match next_2 {
                s @ b"/*" => {
                    nesting += 1;
                    *input = &input[s.len()..];
                }
                s @ b"*/" if nesting > 0 => {
                    nesting -= 1;
                    *input = &input[s.len()..];
                }
                s @ b"*/" => {
                    *input = &input[s.len()..];
                    break Ok(());
                }
                _ => {
                    let mut it = input.chars();
                    it.next(); // skip a char
                    *input = it.as_str();
                }
            }
        }
    };
    alt((
        preceded("//", repeat(0.., (not("\n"), any)).map(|()| ())),
        preceded("/*", comment_block_rest).map(|()| ()),
    ))
    // .context(StrContext::Label("comment"))
    .take()
}

fn keyword<I>() -> impl Parser<I, I::Slice, Error>
where
    I: Stream + StreamIsPartial + for<'s> Compare<&'s str>,
{
    alt((
        "type",
        "dec",
        "def",
        "chan",
        "let",
        "do",
        "in",
        "begin",
        "unfounded",
        "loop",
        "telltypes",
        "either",
        "recursive",
        "iterative",
        "self",
    ))
    .context(StrContext::Label("keyword"))
}
/*
fn with_loc<'a, O, E>(
    mut parser: impl Parser<Input<'a>, O, E>,
) -> impl Parser<Input<'a>, (O, Loc), E>
where
    E: ParserError<Input<'a>> + ModalError,
{
    move |input: &mut Input<'a>| -> core::result::Result<(O, Loc), E> {
        let loc = match peek(any::<_, E>).parse_next(input) {
            Ok(x) => x.loc.clone(),
            Err(e) => {
                let checkpoint = input.checkpoint();
                input.reset_to_start();
                let Some(last) = input.last() else {
                    return Err(e);
                };
                let res = match last.loc {
                    Loc::Code { line, column } => Loc::Code {
                        line,
                        column: column + 1,
                    },
                    Loc::External => Loc::External,
                };
                input.reset(&checkpoint);
                res
            }
        };
        let out = parser.parse_next(input)?;
        Ok((out, loc))
    }
}
#[allow(dead_code)]
fn with_span<'a, O, E>(
    mut parser: impl Parser<Input<'a>, O, E>,
) -> impl Parser<Input<'a>, (O, core::ops::Range<usize>), E>
where
    E: ParserError<Input<'a>>,
{
    move |input: &mut Input<'a>| -> core::result::Result<(O, core::ops::Range<usize>), E> {
        let last = input.last().cloned();
        let start = peek(any).parse_next(input)?.span.start;
        let out = parser.parse_next(input)?;
        let end = peek::<_, &Token, E, _>(any)
            .parse_next(input)
            .unwrap_or(&last.unwrap()) // if input now empty, use that last token.
            .span
            .end;
        Ok((out, start..end))
    }
}
*/

fn name(input: &mut Input) -> Result<Name> {
    t(TokenKind::Identifier)
        .map(Name::from)
        .context(StrContext::Expected(StrContextValue::CharLiteral('_')))
        .context(StrContext::Expected(StrContextValue::Description("alphabetic")))
        .context(StrContext::Label("name"))
        .parse_next(input)
}

struct ProgramParseError {
    offset: usize,
    error: Error_,
}
impl ProgramParseError {
    fn offset(&self) -> usize {
        self.offset
    }
    fn inner(&self) -> &Error_ {
        &self.error
    }
}

fn program(
    mut input: Input,
) -> std::result::Result<Program<Name, Expression<Name>>, ProgramParseError> {
    pub enum Item<Name, Expr> {
        TypeDef(TypeDef<Name>),
        Declaration(Declaration<Name>),
        Definition(Definition<Name, Expr>, Option<Type<Name>>),
    }

    let parser = repeat(
        0..,
        alt((
            type_def.map(Item::TypeDef),
            declaration.map(Item::Declaration),
            definition.map(|(def, typ)| Item::Definition(def, typ)),
        ))
            .context(StrContext::Label("item")),
    )
        .fold(Program::default, |mut acc, item| {
            match item {
                Item::TypeDef(type_def) => {
                    acc.type_defs.push(type_def);
                }
                Item::Declaration(dec) => {
                    acc.declarations.push(dec);
                }
                Item::Definition(Definition { span, name, expression }, annotation) => {
                    if let Some(typ) = annotation {
                        acc.declarations.push(Declaration { span: span.clone(), name: name.clone(), typ });
                    }
                    acc.definitions.push(Definition { span, name, expression });
                }
            };
            acc
        });

    let start = input.checkpoint();
    (
        parser,
        winnow::combinator::eof
            .context(StrContext::Expected(StrContextValue::StringLiteral("type")))
            .context(StrContext::Expected(StrContextValue::StringLiteral("dec")))
            .context(StrContext::Expected(StrContextValue::StringLiteral("def")))
            .context(StrContext::Expected(StrContextValue::Description(
                "end of file",
            ))),
    )
        .parse_next(&mut input)
        .map(|(x, _eof)| x)
        .map_err(|e| {
            let e = e.into_inner().unwrap_or_else(|_err| {
                panic!("complete parsers should not report `ErrMode::Incomplete(_)`")
            });

            ProgramParseError {
                offset: winnow::stream::Offset::offset_from(&input, &start),
                error: ParserError::append(e, &input, &start),
            }
        })
}

#[derive(Debug, Clone, miette::Diagnostic)]
#[diagnostic(severity(Error))]
pub struct SyntaxError {
    #[label]
    span: SourceSpan,
    // Generate these with the miette! macro.
    // #[related]
    // related: Arc<[miette::ErrReport]>,
    #[help]
    help: String,
}
impl core::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "Syntax error.".fmt(f)
    }
}
impl core::error::Error for SyntaxError {}

pub fn set_miette_hook() {
    _ = miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .unicode(true)
                .color(false)
                // .context_lines(1)
                // .with_cause_chain()
                .build(),
        )
    }));
}

pub fn parse_program(
    input: &str
) -> std::result::Result<Program<Name, Expression<Name>>, SyntaxError> {
    let tokens = lex(&input);
    let e = match program(Input::new(&tokens)) {
        Ok(x) => return Ok(x),
        Err(e) => e,
    };
    // Empty input doesn't error so this won't panic.
    let error_tok = tokens.get(e.offset()).unwrap_or(tokens.last().unwrap()).clone();
    Err(SyntaxError {
        span: SourceSpan::new(SourceOffset::from(error_tok.span.start), {
            match error_tok.span.len() {
                // miette unicode format for 1 length span is a hard-to-notice line, so don't set length to 1.
                x if x == 1 => 0,
                x => x,
            }
        }),
        help: e
            .inner()
            .context
            .iter()
            .map(|x| x.1.to_string().chars().chain(['\n']).collect::<String>())
            .collect::<String>(),
    })
}

fn type_def(input: &mut Input) -> Result<TypeDef<Name>> {
    commit_after(t(TokenKind::Type), (with_loc(name), type_params, t(TokenKind::Eq), typ))
        .map(|((name, loc), type_params, _, typ)| (loc, name, type_params, typ))
        .context(StrContext::Label("type definition"))
        .parse_next(input)
}

fn declaration(input: &mut Input) -> Result<Declaration<Name>> {
    commit_after(t(TokenKind::Dec), (with_loc(name), t(TokenKind::Colon), typ))
        .map(|((name, loc), _, typ)| (loc, name, typ))
        .context(StrContext::Label("declaration"))
        .parse_next(input)
}

fn definition(
    input: &mut Input,
) -> Result<(Definition<Name, Expression<Name>>, Option<Type<Name>>)> {
    commit_after(t(TokenKind::Def), (with_loc(name), annotation, t(TokenKind::Eq), expression))
        .map(|((name, loc), annotation, _, expression)| (loc, name, annotation, expression))
        .context(StrContext::Label("definition"))
        .parse_next(input)
}

fn branches_body<'i, P, O>(
    branch: P,
) -> impl Parser<Input<'i>, IndexMap<Name, O>, Error> + use<'i, P, O>
where
    P: Parser<Input<'i>, O, Error>,
{
    commit_after(
        t(TokenKind::LCrl),
        terminated(
            repeat(0.., (t(TokenKind::Dot), name, cut_err(branch), opt(t(TokenKind::Comma)))).fold(
                || IndexMap::new(),
                |mut branches, (_, name, branch, _)| {
                    branches.insert(name, branch);
                    branches
                },
            ),
            t(TokenKind::RCrl),
        ),
    )
    .context(StrContext::Label("either/choice branches"))
}

fn typ(input: &mut Input) -> Result<Type<Name>> {
    alt((
        typ_name,
        typ_chan,
        typ_either,
        typ_choice,
        typ_break,
        typ_continue,
        typ_recursive,
        typ_iterative,
        typ_self,
        typ_send_type,
        typ_send, // try after send_type so matching `(` is unambiguous
        typ_recv_type,
        typ_receive, // try after recv_type so matching `[` is unambiguous
    ))
    .context(StrContext::Label("type"))
    .parse_next(input)
}

fn typ_name(input: &mut Input) -> Result<Type<Name>> {
    trace(
        "typ_name",
        with_loc((name, type_args)).map(|((name, typ_args), loc)| Type::Name(loc, name, typ_args)),
    )
    .parse_next(input)
}

fn typ_chan(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(
        t(TokenKind::Chan),
        typ.context(StrContext::Label("chan type")),
    ))
    .map(|(typ, loc)| Type::Chan(Loc::from(loc), Box::new(typ)))
    .parse_next(input)
}

fn typ_send(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (terminated(list(typ), t(TokenKind::RPar)), typ)))
        .map(|((args, then), span)| {
            args.into_iter().rev().fold(then, |then, arg| {
                Type::Send(Loc::from(span.clone()), Box::new(arg), Box::new(then))
            })
        })
        .parse_next(input)
}

fn typ_receive(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(t(TokenKind::LBkt), (terminated(list(typ), t(TokenKind::RBkt)), typ)))
        .map(|((args, then), span)| {
            args.into_iter().rev().fold(then, |then, arg| {
                Type::Receive(Loc::from(span.clone()), Box::new(arg), Box::new(then))
            })
        })
        .parse_next(input)
}

fn typ_either(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(t(TokenKind::Either), branches_body(typ)))
        .map(|(branches, span)| Type::Either(Loc::from(span), branches))
        .parse_next(input)
}

fn typ_choice(input: &mut Input) -> Result<Type<Name>> {
    with_loc(branches_body(typ_branch))
        .map(|(branches, span)| Type::Choice(Loc::from(span), branches))
        .parse_next(input)
}

fn typ_break(input: &mut Input) -> Result<Type<Name>> {
    with_loc(t(TokenKind::Bang))
        .map(|(_, span)| Type::Break(Loc::from(span)))
        .parse_next(input)
}

fn typ_continue(input: &mut Input) -> Result<Type<Name>> {
    with_loc(t(TokenKind::Quest))
        .map(|(_, span)| Type::Continue(Loc::from(span)))
        .parse_next(input)
}

fn typ_recursive(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(t(TokenKind::Recursive), (loop_label, typ)))
        .map(|((label, typ), loc)| {
            Type::Recursive(Loc::from(loc), Default::default(), label, Box::new(typ))
        })
        .parse_next(input)
}

fn typ_iterative<'s>(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(
        t(TokenKind::Iterative),
        (loop_label, typ).context(StrContext::Label("iterative type body")),
    ))
    .map(|((name, typ), span)| {
        Type::Iterative(Loc::from(span), Default::default(), name, Box::new(typ))
    })
    .parse_next(input)
}

fn typ_self<'s>(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(
        t(TokenKind::Self_),
        loop_label.context(StrContext::Label("self type loop label")),
    ))
    .map(|(label, span)| Type::Self_(Loc::from(span), label))
    .parse_next(input)
}

fn typ_send_type<'s>(input: &mut Input) -> Result<Type<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        (
            list(name).context(StrContext::Label("list of type names to send")),
            t(TokenKind::RPar),
            typ,
        ),
    ))
    .map(|((names, _, typ), span)| {
        names.into_iter().rev().fold(typ, |body, name| {
            Type::SendTypes(Loc::from(span.clone()), name, Box::new(body))
        })
    })
    .parse_next(input)
}

fn typ_recv_type<'s>(input: &mut Input<'s>) -> Result<Type<Name>> {
    with_loc(commit_after(
        tn!("[type": TokenKind::LBkt, TokenKind::Type),
        (
            list(name).context(StrContext::Label("list of type names to receive")),
            t(TokenKind::RBkt),
            typ,
        ),
    ))
    .map(|((names, _, typ), span)| {
        names.into_iter().rev().fold(typ, |body, name| {
            Type::ReceiveType(Loc::from(span.clone()), name, Box::new(body))
        })
    })
    .parse_next(input)
}

fn type_params<'s>(input: &mut Input) -> Result<Vec<Name>> {
    opt(delimited(t(TokenKind::Lt), list(name), t(TokenKind::Gt))) // TODO should be able to use `<` to improve error message
        .map(Option::unwrap_or_default)
        .parse_next(input)
}

fn type_args<'s>(input: &mut Input) -> Result<Vec<Type<Name>>> {
    opt(delimited(t(TokenKind::Lt), list(typ), t(TokenKind::Gt))) // TODO should be able to use `<` to improve error message
        .map(Option::unwrap_or_default)
        .parse_next(input)
}

fn typ_branch<'s>(input: &mut Input<'s>) -> Result<Type<Name>> {
    // try recv_type first so `(` is unambiguous on `typ_branch_received`
    alt((typ_branch_then, typ_branch_recv_type, typ_branch_receive)).parse_next(input)
}

fn typ_branch_then<'s>(input: &mut Input<'s>) -> Result<Type<Name>> {
    commit_after(t(TokenKind::Arrow), typ).parse_next(input)
}

fn typ_branch_receive<'s>(input: &mut Input<'s>) -> Result<Type<Name>> {
    with_loc(commit_after(t(TokenKind::LPar, (list(typ), t(TokenKind::RPar), typ_branch)))
        .map(|((args, _, then), span)| {
            args.into_iter().rev().fold(then, |acc, arg| {
                Type::Receive(Loc::from(span.clone()), Box::new(arg), Box::new(acc))
            })
        })
        .parse_next(input)
}

fn typ_branch_recv_type<'s>(input: &mut Input<'s>) -> Result<Type<Name>> {
    with_loc(preceded(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        cut_err((list(name), t(TokenKind::RPar), typ_branch)),
    ))
    .map(|((names, _, body), span)| {
        names.into_iter().rev().fold(body, |acc, name| {
            Type::ReceiveType(Loc::from(span.clone()), name, Box::new(acc))
        })
    })
    .parse_next(input)
}

fn annotation(input: &mut Input) -> Result<Option<Type<Name>>> {
    opt(commit_after(t(TokenKind::Colon), typ)).parse_next(input)
}

// pattern           = { pattern_name | pattern_receive | pattern_continue | pattern_recv_type }
fn pattern(input: &mut Input) -> Result<Pattern<Name>> {
    alt((
        pattern_name,
        pattern_receive_type,
        pattern_receive,
        pattern_continue,
    ))
    .parse_next(input)
}

fn pattern_name(input: &mut Input) -> Result<Pattern<Name>> {
    with_loc((name, annotation))
        .map(|((name, annotation), loc)| Pattern::Name(loc, name, annotation))
        .parse_next(input)
}

fn pattern_receive(input: &mut Input) -> Result<Pattern<Name>> {
    with_loc(commit_after(t(TokenKind::LPar, (list(pattern), t(TokenKind::RPar), pattern)))
        .map(|((patterns, _, mut rest), loc)| {
            for pattern in patterns.into_iter().rev() {
                rest = Pattern::Receive(loc.clone(), Box::new(pattern), Box::new(rest));
            }
            rest
        })
        .parse_next(input)
}

fn pattern_continue(input: &mut Input) -> Result<Pattern<Name>> {
    with_loc(t(TokenKind::Bang))
        .map(|(_, loc)| Pattern::Continue(loc))
        .parse_next(input)
}

fn pattern_receive_type(input: &mut Input) -> Result<Pattern<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        (list(name), t(TokenKind::RPar), pattern),
    ))
    .map(|((names, _, mut rest), loc)| {
        for name in names.into_iter().rev() {
            rest = Pattern::ReceiveType(loc.clone(), name, Box::new(rest));
        }
        rest
    })
    .parse_next(input)
}

fn expression(input: &mut Input) -> Result<Expression<Name>> {
    alt((
        expr_let,
        expr_do,
        expr_fork,
        application,
        with_loc(construction).map(|(cons, loc)| Expression::Construction(loc, cons)),
        delimited(t(TokenKind::LCrl), expression, t(TokenKind::RCrl)),
    ))
    .context(StrContext::Label("expression"))
    .parse_next(input)
}

fn expr_let(input: &mut Input) -> Result<Expression<Name>> {
    with_loc(commit_after(
        t(TokenKind::Let),
        (pattern, t(TokenKind::Eq), expression, t(TokenKind::In), expression),
    ))
    .map(|((pattern, _, expression, _, body), loc)| {
        Expression::Let(loc, pattern, Box::new(expression), Box::new(body))
    })
    .parse_next(input)
}

fn expr_do(input: &mut Input) -> Result<Expression<Name>> {
    with_loc(commit_after(
        t(TokenKind::Do),
        (t(TokenKind::LCrl), process, (t(TokenKind::RCrl), t(TokenKind::In)), expression),
    ))
    .map(|((_, process, _, expression), loc)| {
        Expression::Do(loc, Box::new(process), Box::new(expression))
    })
    .parse_next(input)
}

fn expr_fork(input: &mut Input) -> Result<Expression<Name>> {
    commit_after(
        t(TokenKind::Chan),
        (with_loc(name), annotation, t(TokenKind::LCrl), process, t(TokenKind::RCrl)),
    )
    .map(|((name, loc), annotation, _, process, _)| {
        Expression::Fork(loc, name, annotation, Box::new(process))
    })
    .parse_next(input)
}

fn construction(input: &mut Input) -> Result<Construct<Name>> {
    alt((
        cons_begin,
        cons_loop,
        cons_then,
        cons_choose,
        cons_either,
        cons_break,
        cons_send_type,
        cons_send,
        cons_recv_type,
        cons_receive,
    ))
    .context(StrContext::Label("construction"))
    .parse_next(input)
}

fn cons_then(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(alt((
        expr_fork,
        expr_let,
        expr_do,
        application,
        delimited(t(TokenKind::LCrl), expression, t(TokenKind::RCrl)),
    )))
    .map(|(expr, loc)| Construct::Then(loc, Box::new(expr)))
    .parse_next(input)
}

fn cons_send(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(commit_after(
        t(TokenKind::LPar),
        (list(expression), t(TokenKind::RPar), construction),
    ))
    .map(|((arguments, _, mut construct), loc)| {
        for argument in arguments.into_iter().rev() {
            construct = Construct::Send(loc.clone(), Box::new(argument), Box::new(construct));
        }
        construct
    })
    .parse_next(input)
}

fn cons_receive(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(commit_after(t(TokenKind::LBkt), (list(pattern), t(TokenKind::RBkt), construction)))
        .map(|((patterns, _, mut construct), loc)| {
            for pattern in patterns.into_iter().rev() {
                construct = Construct::Receive(loc.clone(), pattern, Box::new(construct));
            }
            construct
        })
        .parse_next(input)
}

fn cons_choose(input: &mut Input) -> Result<Construct<Name>> {
    // Note this can't be a commit_after because its possible that this is not a choose construction, and instead a branch of an either.
    with_loc(preceded(t(TokenKind::Dot), (name, construction)))
        .map(|((chosen, construct), loc)| Construct::Choose(loc, chosen, Box::new(construct)))
        .parse_next(input)
}

fn cons_either(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(branches_body(cons_branch))
        .map(|(branches, loc)| Construct::Either(loc, ConstructBranches(branches)))
        .parse_next(input)
}

fn cons_break(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(t(TokenKind::Bang))
        .map(|(_, loc)| Construct::Break(loc))
        .parse_next(input)
}

fn cons_begin(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(opt_commit_after(
        t(TokenKind::Unfounded),
        commit_after(t(TokenKind::Begin), (loop_label, construction)),
    ))
    .map(|((unfounded, (label, construct)), loc)| {
        Construct::Begin(loc, unfounded.is_some(), label, Box::new(construct))
    })
    .parse_next(input)
}

fn cons_loop(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(commit_after(t(TokenKind::Loop), loop_label))
        .map(|(label, loc)| (Construct::Loop(loc, label)))
        .parse_next(input)
}

fn cons_send_type(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        (list(typ), t(TokenKind::RPar), construction),
    ))
    .map(|((names, _, mut construct), loc)| {
        for name in names.into_iter().rev() {
            construct = Construct::SendType(loc.clone(), name, Box::new(construct));
        }
        construct
    })
    .parse_next(input)
}

fn cons_recv_type(input: &mut Input) -> Result<Construct<Name>> {
    with_loc(commit_after(
        tn!("[type": TokenKind::LBkt, TokenKind::Type),
        (list(name), t(TokenKind::RBkt), construction),
    ))
    .map(|((names, _, mut construct), loc)| {
        for name in names.into_iter().rev() {
            construct = Construct::ReceiveType(loc.clone(), name, Box::new(construct));
        }
        construct
    })
    .parse_next(input)
}

fn cons_branch(input: &mut Input) -> Result<ConstructBranch<Name>> {
    alt((cons_branch_then, cons_branch_recv_type, cons_branch_receive)).parse_next(input)
}

fn cons_branch_then(input: &mut Input) -> Result<ConstructBranch<Name>> {
    with_loc(commit_after(t(TokenKind::Arrow), expression))
        .map(|(expression, loc)| ConstructBranch::Then(loc, expression))
        .parse_next(input)
}

fn cons_branch_receive(input: &mut Input) -> Result<ConstructBranch<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (list(pattern), t(TokenKind::RPar), cons_branch)))
        .map(|((patterns, _, mut branch), loc)| {
            for pattern in patterns.into_iter().rev() {
                branch = ConstructBranch::Receive(loc.clone(), pattern, Box::new(branch));
            }
            branch
        })
        .parse_next(input)
}

fn cons_branch_recv_type(input: &mut Input) -> Result<ConstructBranch<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        (list(name), t(TokenKind::RPar), cons_branch),
    ))
    .map(|((names, _, mut branch), loc)| {
        for name in names.into_iter().rev() {
            branch = ConstructBranch::ReceiveType(loc.clone(), name, Box::new(branch));
        }
        branch
    })
    .parse_next(input)
}

fn application(input: &mut Input) -> Result<Expression<Name>> {
    with_loc((
        alt((
            with_loc(name).map(|(name, loc)| Expression::Reference(loc, name)),
            delimited(t(TokenKind::LCrl), expression, t(TokenKind::RCrl)),
        )),
        apply,
    ))
    .map(|((expr, apply), loc)| Expression::Application(loc, Box::new(expr), apply))
    .context(StrContext::Label("application"))
    .parse_next(input)
}

fn apply(input: &mut Input) -> Result<Apply<Name>> {
    alt((
        apply_begin,
        apply_loop,
        apply_choose,
        apply_either,
        apply_send_type,
        apply_send,
        apply_noop,
    ))
    .parse_next(input)
}

fn apply_send(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (list(expression), t(TokenKind::RPar), apply)))
        .map(|((arguments, _, mut apply), loc)| {
            for argument in arguments.into_iter().rev() {
                apply = Apply::Send(loc.clone(), Box::new(argument), Box::new(apply));
            }
            apply
        })
        .parse_next(input)
}

fn apply_choose(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(commit_after(t(TokenKind::Dot), (name, apply)))
        .map(|((chosen, then), loc)| Apply::Choose(loc, chosen, Box::new(then)))
        .parse_next(input)
}

fn apply_either(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(branches_body(apply_branch))
        .map(|(branches, loc)| Apply::Either(loc, ApplyBranches(branches)))
        .parse_next(input)
}

fn apply_begin(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(opt_commit_after(
        t(TokenKind::Unfounded),
        commit_after(t(TokenKind::Begin), (loop_label, apply)),
    ))
    .map(|((unfounded, (label, then)), loc)| {
        Apply::Begin(loc, unfounded.is_some(), label, Box::new(then))
    })
    .parse_next(input)
}

fn apply_loop(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(commit_after(t(TokenKind::Loop), loop_label))
        .map(|(label, loc)| Apply::Loop(loc, label))
        .parse_next(input)
}

fn apply_send_type(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type), (list(typ), t(TokenKind::RPar), apply)))
        .map(|((types, _, mut apply), loc)| {
            for typ in types.into_iter().rev() {
                apply = Apply::SendType(loc.clone(), typ, Box::new(apply));
            }
            apply
        })
        .parse_next(input)
}

fn apply_noop(input: &mut Input) -> Result<Apply<Name>> {
    with_loc(empty)
        .map(|((), loc)| Apply::Noop(loc))
        .parse_next(input)
}

fn apply_branch(input: &mut Input) -> Result<ApplyBranch<Name>> {
    alt((
        apply_branch_then,
        apply_branch_recv_type,
        apply_branch_receive,
        apply_branch_continue,
    ))
    .parse_next(input)
}

fn apply_branch_then(input: &mut Input) -> Result<ApplyBranch<Name>> {
    (with_loc(name), cut_err((t(TokenKind::Arrow), expression)))
        .map(|((name, loc), (_, expression))| ApplyBranch::Then(loc, name, expression))
        .parse_next(input)
}

fn apply_branch_receive(input: &mut Input) -> Result<ApplyBranch<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (list(pattern), t(TokenKind::RPar), apply_branch)))
        .map(|((patterns, _, mut branch), loc)| {
            for pattern in patterns.into_iter().rev() {
                branch = ApplyBranch::Receive(loc.clone(), pattern, Box::new(branch));
            }
            branch
        })
        .parse_next(input)
}

fn apply_branch_continue(input: &mut Input) -> Result<ApplyBranch<Name>> {
    with_loc(commit_after(t(TokenKind::Bang), (t(TokenKind::Arrow), expression)))
        .map(|((_, expression), loc)| ApplyBranch::Continue(loc, expression))
        .parse_next(input)
}

fn apply_branch_recv_type(input: &mut Input) -> Result<ApplyBranch<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type),
        (list(name), t(TokenKind::RPar), apply_branch),
    ))
    .map(|((names, _, mut branch), loc)| {
        for name in names.into_iter().rev() {
            branch = ApplyBranch::ReceiveType(loc.clone(), name, Box::new(branch))
        }
        branch
    })
    .parse_next(input)
}

fn process(input: &mut Input) -> Result<Process<Name>> {
    alt((proc_let, proc_telltypes, command, proc_noop))
        .context(StrContext::Label("process"))
        .parse_next(input)
}

fn proc_let(input: &mut Input) -> Result<Process<Name>> {
    with_loc(commit_after(
        t(TokenKind::Let),
        (pattern, t(TokenKind::Eq), expression, process),
    ))
    .map(|((pattern, _, expression, process), loc)| {
        Process::Let(loc, pattern, Box::new(expression), Box::new(process))
    })
    .parse_next(input)
}

fn proc_telltypes(input: &mut Input) -> Result<Process<Name>> {
    with_loc(commit_after(t(TokenKind::Telltypes), process))
        .map(|(process, loc)| Process::Telltypes(loc, Box::new(process)))
        .parse_next(input)
}

fn proc_noop(input: &mut Input) -> Result<Process<Name>> {
    with_loc(empty)
        .map(|((), loc)| Process::Noop(loc))
        .parse_next(input)
}

fn command(input: &mut Input) -> Result<Process<Name>> {
    (name, cmd)
        .map(|(name, cmd)| Process::Command(name, cmd))
        .parse_next(input)
}

fn cmd(input: &mut Input) -> Result<Command<Name>> {
    alt((
        cmd_link,
        cmd_choose,
        cmd_either,
        cmd_break,
        cmd_continue,
        cmd_begin,
        cmd_loop,
        cmd_send_type,
        cmd_send,
        cmd_recv_type,
        cmd_receive,
        cmd_then,
    ))
    .context(StrContext::Label("command"))
    .parse_next(input)
}

fn cmd_then(input: &mut Input) -> Result<Command<Name>> {
    process
        .map(|x| Command::Then(Box::new(x)))
        .parse_next(input)
}

fn cmd_link(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(t(TokenKind::Link), expression))
        .map(|(expression, loc)| Command::Link(loc, Box::new(expression)))
        .parse_next(input)
}

fn cmd_send(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (list(expression), t(TokenKind::RPar), cmd)))
        .map(|((expressions, _, mut cmd), loc)| {
            for expression in expressions.into_iter().rev() {
                cmd = Command::Send(loc.clone(), Box::new(expression), Box::new(cmd));
            }
            cmd
        })
        .parse_next(input)
}

fn cmd_receive(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(t(TokenKind::LBkt), (list(pattern), t(TokenKind::RBkt), cmd)))
        .map(|((patterns, _, mut cmd), loc)| {
            for pattern in patterns.into_iter().rev() {
                cmd = Command::Receive(loc.clone(), pattern, Box::new(cmd));
            }
            cmd
        })
        .parse_next(input)
}

fn cmd_choose(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(t(TokenKind::Dot), (name, cmd)))
        .map(|((name, cmd), loc)| Command::Choose(loc, name, Box::new(cmd)))
        .parse_next(input)
}

fn cmd_either(input: &mut Input) -> Result<Command<Name>> {
    with_loc((
        branches_body(cmd_branch).map(CommandBranches),
        opt(pass_process),
    ))
    .map(|((branches, pass_process), loc)| {
        Command::Either(loc, branches, pass_process.map(Box::new))
    })
    .parse_next(input)
}

fn cmd_break(input: &mut Input) -> Result<Command<Name>> {
    with_loc(t(TokenKind::Bang))
        .map(|(_, loc)| Command::Break(loc))
        .parse_next(input)
}

fn cmd_continue(input: &mut Input) -> Result<Command<Name>> {
    with_loc((t(TokenKind::Quest), process))
        .map(|((_, process), loc)| Command::Continue(loc, Box::new(process)))
        .parse_next(input)
}

fn cmd_begin(input: &mut Input) -> Result<Command<Name>> {
    with_loc(opt_commit_after(
        t(TokenKind::Unfounded),
        commit_after(t(TokenKind::Begin), (loop_label, cmd)),
    ))
    .map(|((unfounded, (label, cmd)), loc)| {
        Command::Begin(loc, unfounded.is_some(), label, Box::new(cmd))
    })
    .parse_next(input)
}

fn cmd_loop(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(t(TokenKind::Loop), loop_label))
        .map(|(label, loc)| Command::Loop(loc, label))
        .parse_next(input)
}

fn cmd_send_type(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(
        tn!("(type": TokenKind::LPar, TokenKind::Type), (list(typ), t(TokenKind::RPar), cmd)))
        .map(|((types, _, mut cmd), loc)| {
            for typ in types.into_iter().rev() {
                cmd = Command::SendType(loc.clone(), typ, Box::new(cmd));
            }
            cmd
        })
        .parse_next(input)
}

fn cmd_recv_type(input: &mut Input) -> Result<Command<Name>> {
    with_loc(commit_after(
        tn!("[type": TokenKind::LBkt, TokenKind::Type),(list(name), t(TokenKind::RBkt), cmd)))
        .map(|((names, _, mut cmd), loc)| {
            for name in names.into_iter().rev() {
                cmd = Command::ReceiveType(loc.clone(), name, Box::new(cmd));
            }
            cmd
        })
        .parse_next(input)
}

fn pass_process(input: &mut Input) -> Result<Process<Name>> {
    alt((proc_let, proc_telltypes, command)).parse_next(input)
}

fn cmd_branch(input: &mut Input) -> Result<CommandBranch<Name>> {
    alt((
        cmd_branch_then,
        cmd_branch_continue,
        cmd_branch_recv_type,
        cmd_branch_receive,
    ))
    .parse_next(input)
}

fn cmd_branch_then(input: &mut Input) -> Result<CommandBranch<Name>> {
    commit_after(t(TokenKind::Arrow), (t(TokenKind::LCrl), process, t(TokenKind::RCrl)))
        .map(|(_, process, _)| CommandBranch::Then(process))
        .parse_next(input)
}

fn cmd_branch_receive(input: &mut Input) -> Result<CommandBranch<Name>> {
    with_loc(commit_after(t(TokenKind::LPar), (list(pattern), t(TokenKind::RPar), cmd_branch)))
        .map(|((patterns, _, mut branch), loc)| {
            for pattern in patterns.into_iter().rev() {
                branch = CommandBranch::Receive(loc.clone(), pattern, Box::new(branch));
            }
            branch
        })
        .parse_next(input)
}

fn cmd_branch_continue(input: &mut Input) -> Result<CommandBranch<Name>> {
    with_loc(commit_after(t(TokenKind::Bang), (t(TokenKind::Arrow), t(TokenKind::LCrl), process, t(TokenKind::RCrl))))
        .map(|((_, _, process, _), loc)| CommandBranch::Continue(loc, process))
        .parse_next(input)
}

fn cmd_branch_recv_type(input: &mut Input) -> Result<CommandBranch<Name>> {
    with_loc(commit_after(
        tn!("(", "type"),
        (list(name), t(TokenKind::RPar), cmd_branch),
    ))
    .map(|((names, _, mut branch), loc)| {
        for name in names.into_iter().rev() {
            branch = CommandBranch::ReceiveType(loc.clone(), name, Box::new(branch));
        }
        branch
    })
    .parse_next(input)
}

fn loop_label<'s>(input: &mut Input<'s>) -> Result<Option<Name>> {
    opt(preceded(t(TokenKind::Colon), name)).parse_next(input)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::par::lexer::lex;

    /*
    #[test]
    fn test_list() {
        let mut p = list(TokenKind::ab);
        assert_eq!(p.parse("ab").unwrap(), vec!["ab"]);
        assert_eq!(p.parse("ab,ab,ab").unwrap(), vec!["ab", "ab", "ab"]);
        assert_eq!(p.parse("ab,ab,ab,").unwrap(), vec!["ab", "ab", "ab"]);
        assert!(p.parse("ab,ab,ab,,").is_err());
        assert!(p.parse("ba").is_err());
        let toks = lex("ab_12,asd, asdf3");
        let toks = Input::new(&toks);
        {
            assert_eq!(
                list(name).parse(toks).unwrap(),
                vec![
                    Name {
                        string: "ab_12".to_owned()
                    },
                    Name {
                        string: "asd".to_owned()
                    },
                    Name {
                        string: "asdf3".to_owned()
                    }
                ]
            );
        }
    }
    */
    #[test]
    fn test_loop_label() {
        let toks = lex(":one");
        let toks = Input::new(&toks);
        assert_eq!(
            with_span(loop_label).parse(toks).unwrap(),
            (
                Some(Name {
                    string: "one".to_owned()
                }),
                0..4
            )
        );
    }

    #[test]
    fn test_parse_examples() {
        let input = include_str!("../../examples/sample.par");
        assert!(parse_program(input).is_ok());
        let input = include_str!("../../examples/semigroup_queue.par");
        assert!(parse_program(input).is_ok());
        let input = include_str!("../../examples/rock_paper_scissors.par");
        assert!(parse_program(input).is_ok());
        let input = include_str!("../../examples/flatten.par");
        assert!(parse_program(input).is_ok());
        let input = include_str!("../../examples/fibonacci.par");
        assert!(parse_program(input).is_ok());
        let input = include_str!("../../examples/bubble_sort.par");
        assert!(parse_program(input).is_ok());
        let input = "begin the errors";
        assert!(parse_program(input).is_err());
    }
}
