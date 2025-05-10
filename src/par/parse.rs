use super::{
    language::{
        Apply, ApplyBranch, ApplyBranches, Command, CommandBranch, CommandBranches, Construct,
        ConstructBranch, ConstructBranches, Expression, GlobalName, Pattern, Process,
    },
    lexer::{lex, Input, Token, TokenKind},
    primitive::Primitive,
};
use crate::location::{Span, Spanning};
use crate::par::{
    language::LocalName,
    program::{Declaration, Definition, Module, TypeDef},
    types::Type,
};
use arcstr::Substr;
use core::fmt::Display;
use miette::{SourceOffset, SourceSpan};
use num_bigint::BigInt;
use std::collections::BTreeMap;
use winnow::token::literal;
use winnow::{
    combinator::{alt, cut_err, opt, preceded, repeat, separated, terminated, trace},
    error::{
        AddContext, ContextError, ErrMode, ModalError, ParserError, StrContext, StrContextValue,
    },
    stream::{Accumulate, Stream},
    Parser,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MyError<C = StrContext> {
    context: Vec<(usize, ContextError<C>)>,
}

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
        .context(StrContext::Expected(StrContextValue::StringLiteral(
            kind.expected(),
        )))
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
    terminated(
        separated(1.., item, t(TokenKind::Comma)),
        opt(t(TokenKind::Comma)),
    )
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
    trace("commit_after", (prefix, cut_err(parser)))
}

fn lowercase_identifier(input: &mut Input) -> Result<(Span, String)> {
    literal(TokenKind::LowercaseIdentifier)
        .context(StrContext::Expected(StrContextValue::CharLiteral('_')))
        .context(StrContext::Expected(StrContextValue::Description(
            "lower-case alphabetic",
        )))
        .map(|token: &[Token]| (token[0].span, token[0].raw.to_owned()))
        .parse_next(input)
}

fn uppercase_identifier(input: &mut Input) -> Result<(Span, String)> {
    literal(TokenKind::UppercaseIdentifier)
        .context(StrContext::Expected(StrContextValue::Description(
            "upper-case alphabetic",
        )))
        .map(|token: &[Token]| (token[0].span, token[0].raw.to_owned()))
        .parse_next(input)
}

fn local_name(input: &mut Input) -> Result<LocalName> {
    lowercase_identifier
        .map(|(span, string)| LocalName { span, string })
        .parse_next(input)
}

fn global_name(input: &mut Input) -> Result<GlobalName> {
    (
        opt((uppercase_identifier, t(TokenKind::Dot))),
        uppercase_identifier,
    )
        .map(|(opt_module, (mut span, primary))| {
            let module = match opt_module {
                Some(((module_span, module), _)) => {
                    span = module_span.join(span);
                    Some(module)
                }
                None => None,
            };
            GlobalName {
                span,
                module,
                primary,
            }
        })
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

fn program(mut input: Input) -> std::result::Result<Module<Expression>, ProgramParseError> {
    enum Item<Expr> {
        TypeDef(TypeDef),
        Declaration(Declaration),
        Definition(Definition<Expr>, Option<Type>),
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
    .fold(Module::default, |mut acc, item| {
        match item {
            Item::TypeDef(type_def) => {
                acc.type_defs.push(type_def);
            }
            Item::Declaration(dec) => {
                acc.declarations.push(dec);
            }
            Item::Definition(
                Definition {
                    span,
                    name,
                    expression,
                },
                annotation,
            ) => {
                if let Some(typ) = annotation {
                    acc.declarations.push(Declaration {
                        span: span.clone(),
                        name: name.clone(),
                        typ,
                    });
                }
                acc.definitions.push(Definition {
                    span,
                    name,
                    expression,
                });
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
    source_span: SourceSpan,
    // Generate these with the miette! macro.
    // #[related]
    // related: Arc<[miette::ErrReport]>,
    #[help]
    help: String,

    span: Span,
}

impl Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Syntax error")
    }
}
impl core::error::Error for SyntaxError {}

impl Spanning for SyntaxError {
    fn span(&self) -> Span {
        self.span.clone()
    }
}

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

pub fn parse_program(input: &str) -> std::result::Result<Module<Expression>, SyntaxError> {
    let tokens = lex(&input);
    let e = match program(Input::new(&tokens)) {
        Ok(x) => return Ok(x),
        Err(e) => e,
    };
    // Empty input doesn't error so this won't panic.
    let error_tok = tokens
        .get(e.offset())
        .unwrap_or(tokens.last().unwrap())
        .clone();
    let error_tok_span = error_tok.span.clone();
    Err(SyntaxError {
        span: error_tok_span,
        source_span: match error_tok_span {
            Span::None => SourceSpan::new(SourceOffset::from(0), input.len()),
            span @ Span::At { start, .. } => SourceSpan::new(
                SourceOffset::from(start.offset),
                if span.len() == 1 {
                    // miette unicode format for 1 length span is a hard-to-notice line, so don't set length to 1.
                    0
                } else {
                    span.len()
                },
            ),
        },
        help: e
            .inner()
            .context
            .iter()
            .map(|x| x.1.to_string().chars().chain(['\n']).collect::<String>())
            .collect::<String>(),
    })
}

fn type_def(input: &mut Input) -> Result<TypeDef> {
    commit_after(
        t(TokenKind::Type),
        (global_name, type_params, t(TokenKind::Eq), typ),
    )
    .map(|(pre, (name, type_params, _, typ))| TypeDef {
        span: pre.span.join(typ.span()),
        name,
        params: type_params.map_or_else(Vec::new, |(_, params)| params),
        typ,
    })
    .context(StrContext::Label("type definition"))
    .parse_next(input)
}

fn declaration(input: &mut Input) -> Result<Declaration> {
    commit_after(t(TokenKind::Dec), (global_name, t(TokenKind::Colon), typ))
        .map(|(pre, (name, _, typ))| Declaration {
            span: pre.span.join(typ.span()),
            name,
            typ,
        })
        .context(StrContext::Label("declaration"))
        .parse_next(input)
}

fn definition(input: &mut Input) -> Result<(Definition<Expression>, Option<Type>)> {
    commit_after(
        t(TokenKind::Def),
        (global_name, annotation, t(TokenKind::Eq), expression),
    )
    .map(|(pre, (name, annotation, _, expression))| {
        (
            Definition {
                span: pre.span.join(expression.span()),
                name,
                expression,
            },
            annotation,
        )
    })
    .context(StrContext::Label("definition"))
    .parse_next(input)
}

fn branches_body<'i, P, O>(
    branch: P,
) -> impl Parser<Input<'i>, (Span, BTreeMap<LocalName, O>), Error> + use<'i, P, O>
where
    P: Parser<Input<'i>, O, Error>,
{
    commit_after(
        t(TokenKind::LCurly),
        (
            repeat(
                0..,
                (
                    t(TokenKind::Dot),
                    local_name,
                    cut_err(branch),
                    opt(t(TokenKind::Comma)),
                ),
            )
            .fold(
                || BTreeMap::new(),
                |mut branches, (_, name, branch, _)| {
                    branches.insert(name, branch);
                    branches
                },
            ),
            t(TokenKind::RCurly),
        ),
    )
    .map(|(open, (branches, close))| (open.span.join(close.span), branches))
    .context(StrContext::Label("either/choice branches"))
}

fn typ(input: &mut Input) -> Result<Type> {
    alt((
        typ_var,
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

fn typ_var(input: &mut Input) -> Result<Type> {
    trace(
        "typ_var",
        local_name.map(|name| Type::Var(name.span.clone(), name)),
    )
    .parse_next(input)
}

fn typ_name(input: &mut Input) -> Result<Type> {
    trace(
        "typ_name",
        (global_name, type_args).map(|(name, type_args)| match type_args {
            Some((type_args_span, type_args)) => {
                Type::Name(name.span.join(type_args_span), name, type_args)
            }
            None => Type::Name(name.span.clone(), name, vec![]),
        }),
    )
    .parse_next(input)
}

fn typ_chan(input: &mut Input) -> Result<Type> {
    commit_after(
        t(TokenKind::Chan),
        typ.context(StrContext::Label("chan type")),
    )
    .map(|(pre, typ)| Type::Chan(pre.span.join(typ.span()), Box::new(typ)))
    .parse_next(input)
}

fn typ_send(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::LParen), (list(typ), t(TokenKind::RParen), typ))
        .map(|(open, (args, _, then))| {
            let span = open.span.join(then.span());
            args.into_iter().rfold(then, |then, arg| {
                Type::Pair(span, Box::new(arg), Box::new(then))
            })
        })
        .parse_next(input)
}

fn typ_receive(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::LBrack), (list(typ), t(TokenKind::RBrack), typ))
        .map(|(open, (args, _, then))| {
            let span = open.span.join(then.span());
            args.into_iter().rfold(then, |then, arg| {
                Type::Function(span, Box::new(arg), Box::new(then))
            })
        })
        .parse_next(input)
}

fn typ_either(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::Either), branches_body(typ))
        .map(|(pre, (branches_span, branches))| {
            Type::Either(pre.span.join(branches_span), branches)
        })
        .parse_next(input)
}

fn typ_choice(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::Choice), branches_body(typ_branch))
        .map(|(pre, (branches_span, branches))| {
            Type::Choice(pre.span.join(branches_span), branches)
        })
        .parse_next(input)
}

fn typ_break(input: &mut Input) -> Result<Type> {
    t(TokenKind::Bang)
        .map(|token| Type::Break(token.span))
        .parse_next(input)
}

fn typ_continue(input: &mut Input) -> Result<Type> {
    t(TokenKind::Quest)
        .map(|token| Type::Continue(token.span))
        .parse_next(input)
}

fn typ_recursive(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::Recursive), (loop_label, typ))
        .map(|(pre, (label, typ))| Type::Recursive {
            span: pre.span.join(typ.span()),
            asc: Default::default(),
            label,
            body: Box::new(typ),
        })
        .parse_next(input)
}

fn typ_iterative(input: &mut Input) -> Result<Type> {
    commit_after(
        t(TokenKind::Iterative),
        (loop_label, typ).context(StrContext::Label("iterative type body")),
    )
    .map(|(pre, (label, typ))| Type::Iterative {
        span: pre.span.join(typ.span()),
        asc: Default::default(),
        label,
        body: Box::new(typ),
    })
    .parse_next(input)
}

fn typ_self(input: &mut Input) -> Result<Type> {
    commit_after(
        t(TokenKind::Self_),
        loop_label.context(StrContext::Label("self type loop label")),
    )
    .map(|(token, label)| {
        Type::Self_(
            match &label {
                Some(label) => token.span.join(label.span),
                None => token.span.clone(),
            },
            label,
        )
    })
    .parse_next(input)
}

fn typ_send_type<'s>(input: &mut Input) -> Result<Type> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (
            list(local_name).context(StrContext::Label("list of type names to send")),
            t(TokenKind::RParen),
            typ,
        ),
    )
    .map(|((open, _), (names, _, then))| {
        let span = open.span.join(then.span());
        names
            .into_iter()
            .rfold(then, |then, name| Type::Exists(span, name, Box::new(then)))
    })
    .parse_next(input)
}

fn typ_recv_type(input: &mut Input) -> Result<Type> {
    commit_after(
        tn!("[type": TokenKind::LBrack, TokenKind::Type),
        (
            list(local_name).context(StrContext::Label("list of type names to receive")),
            t(TokenKind::RBrack),
            typ,
        ),
    )
    .map(|((open, _), (names, _, then))| {
        let span = open.span.join(then.span());
        names
            .into_iter()
            .rfold(then, |then, name| Type::Forall(span, name, Box::new(then)))
    })
    .parse_next(input)
}

fn type_params(input: &mut Input) -> Result<Option<(Span, Vec<LocalName>)>> {
    // TODO should be able to use `<` to improve error message
    opt((t(TokenKind::Lt), list(local_name), t(TokenKind::Gt)))
        .map(|opt| opt.map(|(open, names, close)| (open.span.join(close.span), names)))
        .parse_next(input)
}

fn type_args<'s>(input: &mut Input) -> Result<Option<(Span, Vec<Type>)>> {
    // TODO should be able to use `<` to improve error message
    opt((t(TokenKind::Lt), list(typ), t(TokenKind::Gt)))
        .map(|opt| opt.map(|(open, types, close)| (open.span.join(close.span), types)))
        .parse_next(input)
}

fn typ_branch(input: &mut Input) -> Result<Type> {
    // try recv_type first so `(` is unambiguous on `typ_branch_received`
    alt((typ_branch_then, typ_branch_recv_type, typ_branch_receive)).parse_next(input)
}

fn typ_branch_then(input: &mut Input) -> Result<Type> {
    commit_after(t(TokenKind::Arrow), typ)
        .map(|(_, typ)| typ)
        .parse_next(input)
}

fn typ_branch_receive(input: &mut Input) -> Result<Type> {
    commit_after(
        t(TokenKind::LParen),
        (list(typ), t(TokenKind::RParen), typ_branch),
    )
    .map(|(open, (args, _, then))| {
        let span = open.span.join(then.span());
        args.into_iter().rfold(then, |then, arg| {
            Type::Function(span, Box::new(arg), Box::new(then))
        })
    })
    .parse_next(input)
}

fn typ_branch_recv_type(input: &mut Input) -> Result<Type> {
    (
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        cut_err((list(local_name), t(TokenKind::RParen), typ_branch)),
    )
        .map(|((open, _), (names, _, then))| {
            let span = open.span.join(then.span());
            names
                .into_iter()
                .rfold(then, |then, name| Type::Forall(span, name, Box::new(then)))
        })
        .parse_next(input)
}

fn annotation(input: &mut Input) -> Result<Option<Type>> {
    opt(commit_after(t(TokenKind::Colon), typ))
        .map(|opt| opt.map(|(_, typ)| typ))
        .parse_next(input)
}

// pattern           = { pattern_name | pattern_receive | pattern_continue | pattern_recv_type }
fn pattern(input: &mut Input) -> Result<Pattern> {
    alt((
        pattern_name,
        pattern_receive_type,
        pattern_receive,
        pattern_continue,
    ))
    .parse_next(input)
}

fn pattern_name(input: &mut Input) -> Result<Pattern> {
    (local_name, annotation)
        .map(|(name, annotation)| {
            Pattern::Name(
                match &annotation {
                    Some(typ) => name.span.join(typ.span()),
                    None => name.span.clone(),
                },
                name,
                annotation,
            )
        })
        .parse_next(input)
}

fn pattern_receive(input: &mut Input) -> Result<Pattern> {
    commit_after(
        t(TokenKind::LParen),
        (list(pattern), t(TokenKind::RParen), pattern),
    )
    .map(|(open, (patterns, _, rest))| {
        let span = open.span.join(rest.span());
        patterns.into_iter().rfold(rest, |rest, arg| {
            Pattern::Receive(span, Box::new(arg), Box::new(rest))
        })
    })
    .parse_next(input)
}

fn pattern_continue(input: &mut Input) -> Result<Pattern> {
    t(TokenKind::Bang)
        .map(|token| Pattern::Continue(token.span))
        .parse_next(input)
}

fn pattern_receive_type(input: &mut Input) -> Result<Pattern> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(local_name), t(TokenKind::RParen), pattern),
    )
    .map(|((open, _), (names, _, rest))| {
        let span = open.span.join(rest.span());
        names.into_iter().rfold(rest, |rest, name| {
            Pattern::ReceiveType(span, name, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn expression(input: &mut Input) -> Result<Expression> {
    alt((
        expr_literal,
        expr_let,
        expr_do,
        expr_fork,
        application,
        construction.map(Expression::Construction),
        expr_grouped,
    ))
    .context(StrContext::Label("expression"))
    .parse_next(input)
}

fn expr_grouped(input: &mut Input) -> Result<Expression> {
    (t(TokenKind::LCurly), expression, t(TokenKind::RCurly))
        .map(|(open, expr, close)| Expression::Grouped(open.span.join(close.span), Box::new(expr)))
        .parse_next(input)
}

fn expr_literal(input: &mut Input) -> Result<Expression> {
    alt((expr_literal_int, expr_literal_string)).parse_next(input)
}

fn expr_literal_int(input: &mut Input) -> Result<Expression> {
    t(TokenKind::Integer)
        .map(|token| {
            let s: String = token.raw.chars().filter(|c| *c != '_').collect();
            let i = BigInt::parse_bytes(s.as_bytes(), 10).unwrap();
            Expression::Primitive(token.span, Primitive::Int(i))
        })
        .parse_next(input)
}

fn expr_literal_string(input: &mut Input) -> Result<Expression> {
    t(TokenKind::String)
        .map(|token| {
            let lit = unescaper::unescape(token.raw).unwrap();
            Expression::Primitive(token.span, Primitive::String(Substr::from(lit)))
        })
        .parse_next(input)
}

fn expr_let(input: &mut Input) -> Result<Expression> {
    commit_after(
        t(TokenKind::Let),
        (
            pattern,
            t(TokenKind::Eq),
            expression,
            t(TokenKind::In),
            expression,
        ),
    )
    .map(|(pre, (pattern, _, expression, _, body))| Expression::Let {
        span: pre.span.join(body.span()),
        pattern,
        expression: Box::new(expression),
        then: Box::new(body),
    })
    .parse_next(input)
}

fn expr_do(input: &mut Input) -> Result<Expression> {
    commit_after(
        t(TokenKind::Do),
        (
            t(TokenKind::LCurly),
            opt(process),
            (t(TokenKind::RCurly), t(TokenKind::In)),
            expression,
        ),
    )
    .map(|(pre, (open, process, _, expression))| Expression::Do {
        span: pre.span.join(expression.span()),
        process: match process {
            Some(process) => Box::new(process),
            None => Box::new(Process::Noop(open.span.only_end())),
        },
        then: Box::new(expression),
    })
    .parse_next(input)
}

fn expr_fork(input: &mut Input) -> Result<Expression> {
    commit_after(
        t(TokenKind::Chan),
        (
            local_name,
            annotation,
            t(TokenKind::LCurly),
            opt(process),
            t(TokenKind::RCurly),
        ),
    )
    .map(
        |(pre, (channel, annotation, open, process, close))| Expression::Fork {
            span: pre.span.join(close.span),
            channel,
            annotation,
            process: match process {
                Some(process) => Box::new(process),
                None => Box::new(Process::Noop(open.span.only_end())),
            },
        },
    )
    .parse_next(input)
}

fn construction(input: &mut Input) -> Result<Construct> {
    alt((
        cons_begin,
        cons_unfounded,
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

fn cons_then(input: &mut Input) -> Result<Construct> {
    alt((
        expr_literal_int,
        expr_fork,
        expr_let,
        expr_do,
        application,
        expr_grouped,
    ))
    .map(Box::new)
    .map(Construct::Then)
    .parse_next(input)
}

fn cons_send(input: &mut Input) -> Result<Construct> {
    commit_after(
        t(TokenKind::LParen),
        (list(expression), t(TokenKind::RParen), construction),
    )
    .map(|(open, (args, _, then))| {
        let span = open.span.join(then.span());
        args.into_iter().rfold(then, |then, arg| {
            Construct::Send(span, Box::new(arg), Box::new(then))
        })
    })
    .parse_next(input)
}

fn cons_receive(input: &mut Input) -> Result<Construct> {
    commit_after(
        t(TokenKind::LBrack),
        (list(pattern), t(TokenKind::RBrack), construction),
    )
    .map(|(open, (patterns, _, then))| {
        let span = open.span.join(then.span());
        patterns.into_iter().rfold(then, |then, pattern| {
            Construct::Receive(span, pattern, Box::new(then))
        })
    })
    .parse_next(input)
}

fn cons_choose(input: &mut Input) -> Result<Construct> {
    // Note this can't be a commit_after because its possible that this is not a choose construction, and instead a branch of an either.
    (t(TokenKind::Dot), (local_name, construction))
        .map(|(pre, (chosen, construct))| {
            Construct::Choose(pre.span.join(construct.span()), chosen, Box::new(construct))
        })
        .parse_next(input)
}

fn cons_either(input: &mut Input) -> Result<Construct> {
    commit_after(t(TokenKind::Case), branches_body(cons_branch))
        .map(|(pre, (branches_span, branches))| {
            Construct::Either(pre.span.join(branches_span), ConstructBranches(branches))
        })
        .parse_next(input)
}

fn cons_break(input: &mut Input) -> Result<Construct> {
    t(TokenKind::Bang)
        .map(|token| Construct::Break(token.span))
        .parse_next(input)
}

fn cons_begin(input: &mut Input) -> Result<Construct> {
    commit_after(t(TokenKind::Begin), (loop_label, construction))
        .map(|(unfounded, (label, construct))| Construct::Begin {
            span: unfounded.span.join(construct.span()),
            unfounded: false,
            label,
            then: Box::new(construct),
        })
        .parse_next(input)
}

fn cons_unfounded(input: &mut Input) -> Result<Construct> {
    commit_after(t(TokenKind::Unfounded), (loop_label, construction))
        .map(|(unfounded, (label, construct))| Construct::Begin {
            span: unfounded.span.join(construct.span()),
            unfounded: true,
            label,
            then: Box::new(construct),
        })
        .parse_next(input)
}

fn cons_loop(input: &mut Input) -> Result<Construct> {
    commit_after(t(TokenKind::Loop), loop_label)
        .map(|(token, label)| {
            Construct::Loop(
                match &label {
                    Some(label) => token.span.join(label.span),
                    None => token.span.clone(),
                },
                label,
            )
        })
        .parse_next(input)
}

fn cons_send_type(input: &mut Input) -> Result<Construct> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(typ), t(TokenKind::RParen), construction),
    )
    .map(|((open, _), (types, _, then))| {
        let span = open.span.join(then.span());
        types.into_iter().rfold(then, |then, typ| {
            Construct::SendType(span, typ, Box::new(then))
        })
    })
    .parse_next(input)
}

fn cons_recv_type(input: &mut Input) -> Result<Construct> {
    commit_after(
        tn!("[type": TokenKind::LBrack, TokenKind::Type),
        (list(local_name), t(TokenKind::RBrack), construction),
    )
    .map(|((open, _), (names, _, then))| {
        let span = open.span.join(then.span());
        names.into_iter().rfold(then, |then, name| {
            Construct::ReceiveType(span, name, Box::new(then))
        })
    })
    .parse_next(input)
}

fn cons_branch(input: &mut Input) -> Result<ConstructBranch> {
    alt((cons_branch_then, cons_branch_recv_type, cons_branch_receive)).parse_next(input)
}

fn cons_branch_then(input: &mut Input) -> Result<ConstructBranch> {
    commit_after(t(TokenKind::Arrow), expression)
        .map(|(pre, expression)| {
            ConstructBranch::Then(pre.span.join(expression.span()), expression)
        })
        .parse_next(input)
}

fn cons_branch_receive(input: &mut Input) -> Result<ConstructBranch> {
    commit_after(
        t(TokenKind::LParen),
        (list(pattern), t(TokenKind::RParen), cons_branch),
    )
    .map(|(open, (patterns, _, rest))| {
        let span = open.span.join(rest.span());
        patterns.into_iter().rfold(rest, |rest, pattern| {
            ConstructBranch::Receive(span, pattern, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn cons_branch_recv_type(input: &mut Input) -> Result<ConstructBranch> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(local_name), t(TokenKind::RParen), cons_branch),
    )
    .map(|((open, _), (names, _, rest))| {
        let span = open.span.join(rest.span());
        names.into_iter().rfold(rest, |rest, name| {
            ConstructBranch::ReceiveType(span, name, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn application(input: &mut Input) -> Result<Expression> {
    (
        alt((
            global_name.map(|name| Expression::Global(name.span, name)),
            local_name.map(|name| Expression::Variable(name.span, name)),
            expr_grouped,
        )),
        apply,
    )
        .map(|(expr, apply)| match apply {
            Some(apply) => {
                Expression::Application(expr.span().join(apply.span()), Box::new(expr), apply)
            }
            None => expr,
        })
        .context(StrContext::Label("application"))
        .parse_next(input)
}

fn apply(input: &mut Input) -> Result<Option<Apply>> {
    opt(alt((
        apply_begin,
        apply_unfounded,
        apply_loop,
        apply_choose,
        apply_either,
        apply_send_type,
        apply_send,
    )))
    .parse_next(input)
}

fn apply_send(input: &mut Input) -> Result<Apply> {
    commit_after(
        t(TokenKind::LParen),
        (list(expression), t(TokenKind::RParen), apply),
    )
    .map(|(open, (args, close, then))| {
        let then = match then {
            Some(apply) => apply,
            None => Apply::Noop(close.span.only_end()),
        };
        let span = open.span.join(then.span());
        args.into_iter().rfold(then, |then, arg| {
            Apply::Send(span, Box::new(arg), Box::new(then))
        })
    })
    .parse_next(input)
}

fn apply_choose(input: &mut Input) -> Result<Apply> {
    (t(TokenKind::Dot), (local_name, apply))
        .map(|(pre, (chosen, then))| {
            let then = match then {
                Some(then) => then,
                None => Apply::Noop(chosen.span.only_end()),
            };
            Apply::Choose(pre.span.join(then.span()), chosen, Box::new(then))
        })
        .parse_next(input)
}

fn apply_either(input: &mut Input) -> Result<Apply> {
    commit_after(
        (t(TokenKind::Dot), t(TokenKind::Case)),
        branches_body(apply_branch),
    )
    .map(|((pre, _), (branches_span, branches))| {
        Apply::Either(pre.span.join(branches_span), ApplyBranches(branches))
    })
    .parse_next(input)
}

fn apply_begin(input: &mut Input) -> Result<Apply> {
    commit_after(
        (t(TokenKind::Dot), t(TokenKind::Begin)),
        (loop_label, apply),
    )
    .map(|((pre, _), (label, then))| {
        let then = match (&label, then) {
            (_, Some(then)) => then,
            (Some(label), None) => Apply::Noop(label.span.only_end()),
            (None, None) => Apply::Noop(pre.span.only_end()),
        };
        Apply::Begin {
            span: pre.span.join(then.span()),
            unfounded: false,
            label,
            then: Box::new(then),
        }
    })
    .parse_next(input)
}

fn apply_unfounded(input: &mut Input) -> Result<Apply> {
    commit_after(
        (t(TokenKind::Dot), t(TokenKind::Unfounded)),
        (loop_label, apply),
    )
    .map(|((pre, _), (label, then))| {
        let then = match (&label, then) {
            (_, Some(then)) => then,
            (Some(label), None) => Apply::Noop(label.span.only_end()),
            (None, None) => Apply::Noop(pre.span.only_end()),
        };
        Apply::Begin {
            span: pre.span.join(then.span()),
            unfounded: true,
            label,
            then: Box::new(then),
        }
    })
    .parse_next(input)
}

fn apply_loop(input: &mut Input) -> Result<Apply> {
    commit_after((t(TokenKind::Dot), t(TokenKind::Loop)), loop_label)
        .map(|((pre1, pre2), label)| {
            Apply::Loop(
                match &label {
                    Some(label) => pre1.span.join(label.span),
                    None => pre1.span.join(pre2.span),
                },
                label,
            )
        })
        .parse_next(input)
}

fn apply_send_type(input: &mut Input) -> Result<Apply> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(typ), t(TokenKind::RParen), apply),
    )
    .map(|((open, _), (types, close, then))| {
        let then = match then {
            Some(apply) => apply,
            None => Apply::Noop(close.span.only_end()),
        };
        let span = open.span.join(then.span());
        types
            .into_iter()
            .rfold(then, |then, typ| Apply::SendType(span, typ, Box::new(then)))
    })
    .parse_next(input)
}

fn apply_branch(input: &mut Input) -> Result<ApplyBranch> {
    alt((
        apply_branch_then,
        apply_branch_recv_type,
        apply_branch_receive,
        apply_branch_continue,
    ))
    .parse_next(input)
}

fn apply_branch_then(input: &mut Input) -> Result<ApplyBranch> {
    (local_name, cut_err((t(TokenKind::Arrow), expression)))
        .map(|(name, (_, expression))| {
            ApplyBranch::Then(name.span.join(expression.span()), name, expression)
        })
        .parse_next(input)
}

fn apply_branch_receive(input: &mut Input) -> Result<ApplyBranch> {
    commit_after(
        t(TokenKind::LParen),
        (list(pattern), t(TokenKind::RParen), apply_branch),
    )
    .map(|(open, (patterns, _, rest))| {
        let span = open.span.join(rest.span());
        patterns.into_iter().rfold(rest, |rest, pattern| {
            ApplyBranch::Receive(span, pattern, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn apply_branch_continue(input: &mut Input) -> Result<ApplyBranch> {
    commit_after(t(TokenKind::Bang), (t(TokenKind::Arrow), expression))
        .map(|(token, (_, expression))| {
            ApplyBranch::Continue(token.span.join(expression.span()), expression)
        })
        .parse_next(input)
}

fn apply_branch_recv_type(input: &mut Input) -> Result<ApplyBranch> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(local_name), t(TokenKind::RParen), apply_branch),
    )
    .map(|((open, _), (names, _, rest))| {
        let span = open.span.join(rest.span());
        names.into_iter().rfold(rest, |rest, name| {
            ApplyBranch::ReceiveType(span, name, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn process(input: &mut Input) -> Result<Process> {
    alt((proc_let, proc_telltypes, global_command, command))
        .context(StrContext::Label("process"))
        .parse_next(input)
}

fn proc_let(input: &mut Input) -> Result<Process> {
    commit_after(
        t(TokenKind::Let),
        (pattern, t(TokenKind::Eq), expression, opt(process)),
    )
    .map(|(pre, (pattern, _, expression, process))| Process::Let {
        span: pre.span.join(expression.span()),
        pattern,
        then: match process {
            Some(process) => Box::new(process),
            None => Box::new(Process::Noop(expression.span().only_end())),
        },
        value: Box::new(expression),
    })
    .parse_next(input)
}

fn proc_telltypes(input: &mut Input) -> Result<Process> {
    commit_after(t(TokenKind::Telltypes), opt(process))
        .map(|(token, process)| {
            Process::Telltypes(
                token.span,
                match process {
                    Some(process) => Box::new(process),
                    None => Box::new(Process::Noop(token.span.only_end())),
                },
            )
        })
        .parse_next(input)
}

fn global_command(input: &mut Input) -> Result<Process> {
    (global_name, cmd)
        .map(|(name, cmd)| match cmd {
            Some(cmd) => Process::GlobalCommand(name, cmd),
            None => {
                let noop_span = name.span.only_end();
                Process::GlobalCommand(name, noop_cmd(noop_span))
            }
        })
        .parse_next(input)
}

fn command(input: &mut Input) -> Result<Process> {
    (local_name, cmd)
        .map(|(name, cmd)| match cmd {
            Some(cmd) => Process::Command(name, cmd),
            None => {
                let noop_span = name.span.only_end();
                Process::Command(name, noop_cmd(noop_span))
            }
        })
        .parse_next(input)
}

fn noop_cmd(span: Span) -> Command {
    Command::Then(Box::new(Process::Noop(span)))
}

fn cmd(input: &mut Input) -> Result<Option<Command>> {
    alt((
        alt((
            cmd_link,
            cmd_choose,
            cmd_either,
            cmd_break,
            cmd_continue,
            cmd_begin,
            cmd_unfounded,
            cmd_loop,
            cmd_send_type,
            cmd_send,
            cmd_recv_type,
            cmd_receive,
        ))
        .map(Some),
        cmd_then,
    ))
    .context(StrContext::Label("command"))
    .parse_next(input)
}

fn cmd_then(input: &mut Input) -> Result<Option<Command>> {
    opt(process)
        .map(|opt| opt.map(|process| Command::Then(Box::new(process))))
        .parse_next(input)
}

fn cmd_link(input: &mut Input) -> Result<Command> {
    commit_after(t(TokenKind::Link), expression)
        .map(|(token, expression)| {
            Command::Link(token.span.join(expression.span()), Box::new(expression))
        })
        .parse_next(input)
}

fn cmd_send(input: &mut Input) -> Result<Command> {
    commit_after(
        t(TokenKind::LParen),
        (list(expression), t(TokenKind::RParen), cmd),
    )
    .map(|(open, (expressions, close, cmd))| {
        let cmd = match cmd {
            Some(cmd) => cmd,
            None => noop_cmd(close.span.only_end()),
        };
        let span = open.span.join(cmd.span());
        expressions.into_iter().rfold(cmd, |cmd, expression| {
            Command::Send(span, expression, Box::new(cmd))
        })
    })
    .parse_next(input)
}

fn cmd_receive(input: &mut Input) -> Result<Command> {
    commit_after(
        t(TokenKind::LBrack),
        (list(pattern), t(TokenKind::RBrack), cmd),
    )
    .map(|(open, (patterns, close, cmd))| {
        let cmd = match cmd {
            Some(cmd) => cmd,
            None => noop_cmd(close.span.only_end()),
        };
        let span = open.span.join(cmd.span());
        patterns.into_iter().rfold(cmd, |cmd, pattern| {
            Command::Receive(span, pattern, Box::new(cmd))
        })
    })
    .parse_next(input)
}

fn cmd_choose(input: &mut Input) -> Result<Command> {
    (t(TokenKind::Dot), (local_name, cmd))
        .map(|(pre, (name, cmd))| {
            let cmd = match cmd {
                Some(cmd) => cmd,
                None => noop_cmd(name.span.only_end()),
            };
            Command::Choose(pre.span.join(cmd.span()), name, Box::new(cmd))
        })
        .parse_next(input)
}

fn cmd_either(input: &mut Input) -> Result<Command> {
    commit_after(
        (t(TokenKind::Dot), t(TokenKind::Case)),
        (branches_body(cmd_branch), opt(pass_process)),
    )
    .map(|((pre, _), ((branches_span, branches), pass_process))| {
        Command::Either(
            pre.span.join(branches_span),
            CommandBranches(branches),
            pass_process.map(Box::new),
        )
    })
    .parse_next(input)
}

fn cmd_break(input: &mut Input) -> Result<Command> {
    t(TokenKind::Bang)
        .map(|token| Command::Break(token.span))
        .parse_next(input)
}

fn cmd_continue(input: &mut Input) -> Result<Command> {
    (t(TokenKind::Quest), opt(process))
        .map(|(token, process)| match process {
            Some(process) => Command::Continue(token.span.join(process.span()), Box::new(process)),
            None => Command::Continue(
                token.span.clone(),
                Box::new(Process::Noop(token.span.only_end())),
            ),
        })
        .parse_next(input)
}

fn cmd_begin(input: &mut Input) -> Result<Command> {
    commit_after((t(TokenKind::Dot), t(TokenKind::Begin)), (loop_label, cmd))
        .map(|((pre, _), (label, cmd))| {
            let cmd = match (&label, cmd) {
                (_, Some(cmd)) => cmd,
                (Some(label), None) => noop_cmd(label.span.only_end()),
                (None, None) => noop_cmd(pre.span.only_end()),
            };
            Command::Begin {
                span: pre.span.join(cmd.span()),
                unfounded: false,
                label,
                then: Box::new(cmd),
            }
        })
        .parse_next(input)
}

fn cmd_unfounded(input: &mut Input) -> Result<Command> {
    commit_after(
        (t(TokenKind::Dot), t(TokenKind::Unfounded)),
        (loop_label, cmd),
    )
    .map(|((pre, _), (label, cmd))| {
        let cmd = match (&label, cmd) {
            (_, Some(cmd)) => cmd,
            (Some(label), None) => noop_cmd(label.span.only_end()),
            (None, None) => noop_cmd(pre.span.only_end()),
        };
        Command::Begin {
            span: pre.span.join(cmd.span()),
            unfounded: true,
            label,
            then: Box::new(cmd),
        }
    })
    .parse_next(input)
}

fn cmd_loop(input: &mut Input) -> Result<Command> {
    commit_after((t(TokenKind::Dot), t(TokenKind::Loop)), loop_label)
        .map(|((pre1, pre2), label)| {
            Command::Loop(
                match &label {
                    Some(label) => pre1.span.join(label.span),
                    None => pre1.span.join(pre2.span),
                },
                label,
            )
        })
        .parse_next(input)
}

fn cmd_send_type(input: &mut Input) -> Result<Command> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(typ), t(TokenKind::RParen), cmd),
    )
    .map(|((open, _), (types, close, cmd))| {
        let cmd = match cmd {
            Some(cmd) => cmd,
            None => noop_cmd(close.span.only_end()),
        };
        let span = open.span.join(cmd.span());
        types
            .into_iter()
            .rfold(cmd, |cmd, typ| Command::SendType(span, typ, Box::new(cmd)))
    })
    .parse_next(input)
}

fn cmd_recv_type(input: &mut Input) -> Result<Command> {
    commit_after(
        tn!("[type": TokenKind::LBrack, TokenKind::Type),
        (list(local_name), t(TokenKind::RBrack), cmd),
    )
    .map(|((open, _), (names, close, cmd))| {
        let cmd = match cmd {
            Some(cmd) => cmd,
            None => noop_cmd(close.span.only_end()),
        };
        let span = open.span.join(cmd.span());
        names.into_iter().rfold(cmd, |cmd, name| {
            Command::ReceiveType(span, name, Box::new(cmd))
        })
    })
    .parse_next(input)
}

fn pass_process(input: &mut Input) -> Result<Process> {
    alt((proc_let, proc_telltypes, global_command, command)).parse_next(input)
}

fn cmd_branch(input: &mut Input) -> Result<CommandBranch> {
    alt((
        cmd_branch_then,
        cmd_branch_bind_then,
        cmd_branch_continue,
        cmd_branch_recv_type,
        cmd_branch_receive,
    ))
    .parse_next(input)
}

fn cmd_branch_then(input: &mut Input) -> Result<CommandBranch> {
    commit_after(
        t(TokenKind::Arrow),
        (t(TokenKind::LCurly), opt(process), t(TokenKind::RCurly)),
    )
    .map(|(pre, (open, process, close))| {
        CommandBranch::Then(
            pre.span.join(close.span),
            match process {
                Some(process) => process,
                None => Process::Noop(open.span.only_end()),
            },
        )
    })
    .parse_next(input)
}

fn cmd_branch_bind_then(input: &mut Input) -> Result<CommandBranch> {
    (
        local_name,
        cut_err((
            t(TokenKind::Arrow),
            (t(TokenKind::LCurly), opt(process), t(TokenKind::RCurly)),
        )),
    )
        .map(|(name, (pre, (open, process, close)))| {
            CommandBranch::BindThen(
                pre.span.join(close.span),
                name,
                match process {
                    Some(process) => process,
                    None => Process::Noop(open.span.only_end()),
                },
            )
        })
        .parse_next(input)
}

fn cmd_branch_receive(input: &mut Input) -> Result<CommandBranch> {
    commit_after(
        t(TokenKind::LParen),
        (list(pattern), t(TokenKind::RParen), cmd_branch),
    )
    .map(|(open, (patterns, _, rest))| {
        let span = open.span.join(rest.span());
        patterns.into_iter().rfold(rest, |rest, pattern| {
            CommandBranch::Receive(span, pattern, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn cmd_branch_continue(input: &mut Input) -> Result<CommandBranch> {
    commit_after(
        t(TokenKind::Bang),
        (
            t(TokenKind::Arrow),
            t(TokenKind::LCurly),
            opt(process),
            t(TokenKind::RCurly),
        ),
    )
    .map(|(token, (_, open, process, close))| {
        CommandBranch::Continue(
            token.span.join(close.span),
            match process {
                Some(process) => process,
                None => Process::Noop(open.span.only_end()),
            },
        )
    })
    .parse_next(input)
}

fn cmd_branch_recv_type(input: &mut Input) -> Result<CommandBranch> {
    commit_after(
        tn!("(type": TokenKind::LParen, TokenKind::Type),
        (list(local_name), t(TokenKind::RParen), cmd_branch),
    )
    .map(|((open, _), (names, _, rest))| {
        let span = open.span.join(rest.span());
        names.into_iter().rfold(rest, |rest, name| {
            CommandBranch::ReceiveType(span, name, Box::new(rest))
        })
    })
    .parse_next(input)
}

fn loop_label(input: &mut Input) -> Result<Option<LocalName>> {
    opt(preceded(t(TokenKind::Slash), local_name)).parse_next(input)
}

#[cfg(test)]
mod test {
    use super::*;

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
