// why not rename this file to ast.rs?

use std::{collections::BTreeMap, fmt::Display, hash::Hash, sync::Arc};

use super::{
    primitive::Primitive,
    process::{self, Captures},
    types::Type,
};
use crate::location::{Span, Spanning};

#[derive(Clone, Debug)]
pub struct LocalName {
    pub span: Span,
    pub string: String,
}

#[derive(Clone, Debug)]
pub struct GlobalName {
    pub span: Span,
    pub module: Option<String>,
    pub primary: String,
}

impl GlobalName {
    pub fn external(module: Option<&'static str>, primary: &'static str) -> Self {
        GlobalName {
            span: Default::default(),
            module: module.map(String::from),
            primary: String::from(primary),
        }
    }
}

impl Spanning for LocalName {
    fn span(&self) -> Span {
        self.span.clone()
    }
}

impl Spanning for GlobalName {
    fn span(&self) -> Span {
        self.span.clone()
    }
}

impl LocalName {
    pub fn result() -> Self {
        Self {
            span: Default::default(),
            string: String::from("#result"),
        }
    }

    pub fn object() -> Self {
        Self {
            span: Default::default(),
            string: String::from("#object"),
        }
    }

    pub fn match_(level: usize) -> Self {
        Self {
            span: Default::default(),
            string: format!("#match{}", level),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Name(Span, LocalName, Option<Type>),
    Receive(Span, Box<Self>, Box<Self>),
    Continue(Span),
    ReceiveType(Span, LocalName, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum Expression {
    Primitive(Span, Primitive),
    Global(Span, GlobalName),
    Variable(Span, LocalName),
    Grouped(Span, Box<Self>),
    Let {
        span: Span,
        pattern: Pattern,
        expression: Box<Self>,
        then: Box<Self>,
    },
    Do {
        span: Span,
        process: Box<Process>,
        then: Box<Self>,
    },
    Fork {
        span: Span,
        channel: LocalName,
        annotation: Option<Type>,
        process: Box<Process>,
    },
    Construction(Construct),
    Application(Span, Box<Self>, Apply),
}

#[derive(Clone, Debug)]
pub enum Construct {
    /// wraps an expression
    Then(Box<Expression>),
    Send(Span, Box<Expression>, Box<Self>),
    Receive(Span, Pattern, Box<Self>),
    /// constructs an either type
    Choose(Span, LocalName, Box<Self>),
    /// constructs a choice type
    Either(Span, ConstructBranches),
    /// ! (unit)
    Break(Span),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<LocalName>,
        then: Box<Self>,
    },
    Loop(Span, Option<LocalName>),
    SendType(Span, Type, Box<Self>),
    ReceiveType(Span, LocalName, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct ConstructBranches(pub BTreeMap<LocalName, ConstructBranch>);

#[derive(Clone, Debug)]
pub enum ConstructBranch {
    Then(Span, Expression),
    Receive(Span, Pattern, Box<Self>),
    ReceiveType(Span, LocalName, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum Apply {
    Noop(Span),
    Send(Span, Box<Expression>, Box<Self>),
    Choose(Span, LocalName, Box<Self>),
    Either(Span, ApplyBranches),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<LocalName>,
        then: Box<Self>,
    },
    Loop(Span, Option<LocalName>),
    SendType(Span, Type, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct ApplyBranches(pub BTreeMap<LocalName, ApplyBranch>);

#[derive(Clone, Debug)]
pub enum ApplyBranch {
    Then(Span, LocalName, Expression),
    Receive(Span, Pattern, Box<Self>),
    Continue(Span, Expression),
    ReceiveType(Span, LocalName, Box<Self>),
}

// span doesn't include the "then" process
#[derive(Clone, Debug)]
pub enum Process {
    Let {
        span: Span,
        pattern: Pattern,
        value: Box<Expression>,
        then: Box<Self>,
    },
    GlobalCommand(GlobalName, Command),
    Command(LocalName, Command),
    Telltypes(Span, Box<Self>),
    Noop(Span),
}

#[derive(Clone, Debug)]
pub enum Command {
    Then(Box<Process>),
    Link(Span, Box<Expression>),
    Send(Span, Expression, Box<Self>),
    Receive(Span, Pattern, Box<Self>),
    Choose(Span, LocalName, Box<Self>),
    Either(Span, CommandBranches, Option<Box<Process>>),
    Break(Span),
    Continue(Span, Box<Process>),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<LocalName>,
        then: Box<Self>,
    },
    Loop(Span, Option<LocalName>),
    SendType(Span, Type, Box<Self>),
    ReceiveType(Span, LocalName, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct CommandBranches(pub BTreeMap<LocalName, CommandBranch>);

#[derive(Clone, Debug)]
pub enum CommandBranch {
    Then(Span, Process),
    BindThen(Span, LocalName, Process),
    Receive(Span, Pattern, Box<Self>),
    Continue(Span, Process),
    ReceiveType(Span, LocalName, Box<Self>),
}

impl Hash for LocalName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.string.hash(state);
    }
}
impl PartialEq for LocalName {
    fn eq(&self, other: &Self) -> bool {
        self.string == other.string
    }
}
impl Eq for LocalName {}
impl PartialOrd for LocalName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.string.partial_cmp(&other.string)
    }
}
impl Ord for LocalName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.string.cmp(&other.string)
    }
}
impl Display for LocalName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.string)
    }
}

impl GlobalName {
    pub fn qualify(&mut self, module: &str) {
        let new = match self.module.take() {
            Some(old) => old,
            None => String::from(module),
        };
        self.module = Some(new);
    }

    fn no_module_or_same_as_primary(&self) -> bool {
        if let Some(module) = &self.module {
            module == &self.primary
        } else {
            true
        }
    }
}

impl Hash for GlobalName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.no_module_or_same_as_primary() {
            self.module.hash(state);
        }
        self.primary.hash(state);
    }
}
impl PartialEq for GlobalName {
    fn eq(&self, other: &Self) -> bool {
        if self.no_module_or_same_as_primary() && other.no_module_or_same_as_primary() {
            self.primary == other.primary
        } else {
            (&self.module, &self.primary) == (&other.module, &other.primary)
        }
    }
}
impl Eq for GlobalName {}
impl PartialOrd for GlobalName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.no_module_or_same_as_primary() && other.no_module_or_same_as_primary() {
            self.primary.partial_cmp(&other.primary)
        } else {
            (&self.module, &self.primary).partial_cmp(&(&other.module, &other.primary))
        }
    }
}
impl Ord for GlobalName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.no_module_or_same_as_primary() && other.no_module_or_same_as_primary() {
            self.primary.cmp(&other.primary)
        } else {
            (&self.module, &self.primary).cmp(&(&other.module, &other.primary))
        }
    }
}
impl Display for GlobalName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.no_module_or_same_as_primary() {
            if let Some(module) = &self.module {
                write!(f, "{}.", module)?;
            }
        }
        write!(f, "{}", self.primary)
    }
}

#[derive(Clone, Debug)]
pub enum CompileError {
    MustEndProcess(Span),
}

impl Spanning for CompileError {
    fn span(&self) -> Span {
        match self {
            CompileError::MustEndProcess(span) => span.clone(),
        }
    }
}

type Pass = Option<Arc<process::Process<()>>>;

impl Pattern {
    pub fn compile_let(
        &self,
        span: &Span,
        expression: Arc<process::Expression<()>>,
        process: Arc<process::Process<()>>,
    ) -> Arc<process::Process<()>> {
        if let Self::Name(_, name, annotation) = self {
            return Arc::new(process::Process::Let {
                span: span.clone(),
                name: name.clone(),
                annotation: annotation.clone(),
                typ: (),
                value: expression,
                then: process,
            });
        }
        Arc::new(process::Process::Let {
            span: span.clone(),
            name: LocalName::match_(0),
            annotation: self.annotation(),
            typ: (),
            value: expression,
            then: self.compile_helper(0, process),
        })
    }

    pub fn compile_receive(
        &self,
        level: usize,
        span: &Span,
        subject: &LocalName,
        process: Arc<process::Process<()>>,
    ) -> Arc<process::Process<()>> {
        if let Self::Name(_, name, annotation) = self {
            return Arc::new(process::Process::Do {
                span: span.clone(),
                name: subject.clone(),
                typ: (),
                command: process::Command::Receive(name.clone(), annotation.clone(), (), process),
            });
        }
        Arc::new(process::Process::Do {
            span: span.clone(),
            name: subject.clone(),
            typ: (),
            command: process::Command::Receive(
                LocalName::match_(level),
                self.annotation(),
                (),
                self.compile_helper(level, process),
            ),
        })
    }

    fn compile_helper(
        &self,
        level: usize,
        process: Arc<process::Process<()>>,
    ) -> Arc<process::Process<()>> {
        match self {
            Self::Name(span, name, annotation) => Arc::new(process::Process::Let {
                span: span.clone(),
                name: name.clone(),
                annotation: annotation.clone(),
                typ: (),
                value: Arc::new(process::Expression::Variable(
                    span.clone(),
                    LocalName::match_(level),
                    (),
                )),
                then: process,
            }),

            Self::Receive(span, first, rest) => first.compile_receive(
                level + 1,
                span,
                &LocalName::match_(level),
                rest.compile_helper(level, process),
            ),

            Self::Continue(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::match_(level),
                typ: (),
                command: process::Command::Continue(process),
            }),

            Self::ReceiveType(span, parameter, rest) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::match_(level),
                typ: (),
                command: process::Command::ReceiveType(
                    parameter.clone(),
                    rest.compile_helper(level, process),
                ),
            }),
        }
    }

    fn annotation(&self) -> Option<Type> {
        match self {
            Self::Name(_, _, annotation) => annotation.clone(),
            Self::Receive(span, first, rest) => {
                let first = first.annotation()?;
                let rest = rest.annotation()?;
                Some(Type::Pair(span.clone(), Box::new(first), Box::new(rest)))
            }
            Self::Continue(span) => Some(Type::Break(span.clone())),
            Self::ReceiveType(span, parameter, rest) => {
                let rest = rest.annotation()?;
                Some(Type::Exists(
                    span.clone(),
                    parameter.clone(),
                    Box::new(rest),
                ))
            }
        }
    }
}

impl Spanning for Pattern {
    fn span(&self) -> Span {
        match self {
            Self::Name(span, _, _)
            | Self::Continue(span)
            | Self::Receive(span, _, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

impl Expression {
    pub fn compile(&self) -> Result<Arc<process::Expression<()>>, CompileError> {
        Ok(match self {
            Self::Primitive(span, value) => Arc::new(process::Expression::Primitive(
                span.clone(),
                value.clone(),
                (),
            )),

            Self::Global(span, name) => {
                Arc::new(process::Expression::Global(span.clone(), name.clone(), ()))
            }

            Self::Variable(span, name) => Arc::new(process::Expression::Variable(
                span.clone(),
                name.clone(),
                (),
            )),

            Self::Grouped(_, expression) => expression.compile()?,

            Self::Let {
                span,
                pattern,
                expression,
                then: body,
            } => {
                let expression = expression.compile()?;
                let body = body.compile()?;
                Arc::new(process::Expression::Fork {
                    span: span.clone(),
                    captures: Captures::new(),
                    chan_name: LocalName::result(),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process: pattern.compile_let(
                        span,
                        expression,
                        Arc::new(process::Process::Do {
                            span: span.clone(),
                            name: LocalName::result(),
                            typ: (),
                            command: process::Command::Link(body),
                        }),
                    ),
                })
            }

            Self::Do {
                span,
                process,
                then: expression,
            } => {
                let expression = expression.compile()?;
                let body = process.compile(Some(Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Link(expression),
                })))?;
                Arc::new(process::Expression::Fork {
                    span: span.clone(),
                    captures: Captures::new(),
                    chan_name: LocalName::result(),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process: body,
                })
            }

            Self::Fork {
                span,
                channel,
                annotation,
                process,
            } => Arc::new(process::Expression::Fork {
                span: span.clone(),
                captures: Captures::new(),
                chan_name: channel.clone(),
                chan_annotation: annotation.clone(),
                chan_type: (),
                expr_type: (),
                process: process.compile(None)?,
            }),

            Self::Construction(construct) => {
                let process = construct.compile()?;
                Arc::new(process::Expression::Fork {
                    span: construct.span().clone(),
                    captures: Captures::new(),
                    chan_name: LocalName::result(),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process,
                })
            }

            Self::Application(_, expr, Apply::Noop(_)) => expr.compile()?,

            Self::Application(span, expr, apply) => {
                let expr = expr.compile()?;
                let process = apply.compile()?;
                Arc::new(process::Expression::Fork {
                    span: span.clone(),
                    captures: Captures::new(),
                    chan_name: LocalName::result(),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process: Arc::new(process::Process::Let {
                        span: span.clone(),
                        name: LocalName::object(),
                        annotation: None,
                        typ: (),
                        value: expr,
                        then: process,
                    }),
                })
            }
        })
    }
}

impl Spanning for Expression {
    fn span(&self) -> Span {
        match self {
            Self::Primitive(span, _)
            | Self::Global(span, _)
            | Self::Variable(span, _)
            | Self::Grouped(span, _)
            | Self::Let { span, .. }
            | Self::Do { span, .. }
            | Self::Fork { span, .. }
            | Self::Application(span, _, _) => span.clone(),

            Self::Construction(construction) => construction.span(),
        }
    }
}

impl Construct {
    pub fn compile(&self) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Then(expression) => {
                let span = expression.span().clone();
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span,
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Send(span, argument, construct) => {
                let argument = argument.compile()?;
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Send(argument, process),
                })
            }

            Self::Receive(span, pattern, construct) => {
                let process = construct.compile()?;
                pattern.compile_receive(0, span, &LocalName::result(), process)
            }

            Self::Choose(span, chosen, construct) => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Signal(chosen.clone(), process),
                })
            }

            Self::Either(span, ConstructBranches(construct_branches)) => {
                let mut branches = Vec::new();
                let mut processes = Vec::new();
                for (branch_name, construct_branch) in construct_branches {
                    branches.push(branch_name.clone());
                    processes.push(construct_branch.compile()?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Case(branches, processes),
                })
            }

            Self::Break(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::result(),
                typ: (),
                command: process::Command::Break,
            }),

            Self::Begin {
                span,
                unfounded,
                label,
                then: construct,
            } => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone(),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::result(),
                typ: (),
                command: process::Command::Loop(label.clone(), Captures::new()),
            }),

            Self::SendType(span, argument, construct) => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::SendType(argument.clone(), process),
                })
            }

            Self::ReceiveType(span, parameter, construct) => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::ReceiveType(parameter.clone(), process),
                })
            }
        })
    }
}

impl Spanning for Construct {
    fn span(&self) -> Span {
        match self {
            Self::Send(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Choose(span, _, _)
            | Self::Either(span, _)
            | Self::Break(span)
            | Self::Begin { span, .. }
            | Self::Loop(span, _)
            | Self::SendType(span, _, _)
            | Self::ReceiveType(span, _, _) => span.clone(),

            Self::Then(expression) => expression.span(),
        }
    }
}

impl ConstructBranch {
    pub fn compile(&self) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Then(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile()?;
                pattern.compile_receive(0, span, &LocalName::result(), process)
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::result(),
                    typ: (),
                    command: process::Command::ReceiveType(parameter.clone(), process),
                })
            }
        })
    }
}

impl Spanning for ConstructBranch {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _) | Self::Receive(span, _, _) | Self::ReceiveType(span, _, _) => {
                span.clone()
            }
        }
    }
}

impl Apply {
    pub fn compile(&self) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Noop(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::result(),
                typ: (),
                command: process::Command::Link(Arc::new(process::Expression::Variable(
                    span.clone(),
                    LocalName::object(),
                    (),
                ))),
            }),

            Self::Send(span, expression, apply) => {
                let expression = expression.compile()?;
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::Send(expression, process),
                })
            }

            Self::Choose(span, chosen, apply) => {
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::Signal(chosen.clone(), process),
                })
            }

            Self::Either(span, ApplyBranches(expression_branches)) => {
                let mut branches = Vec::new();
                let mut processes = Vec::new();
                for (branch_name, expression_branch) in expression_branches {
                    branches.push(branch_name.clone());
                    processes.push(expression_branch.compile()?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::Case(branches, processes),
                })
            }

            Self::Begin {
                span,
                unfounded,
                label,
                then: apply,
            } => {
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone(),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: LocalName::object(),
                typ: (),
                command: process::Command::Loop(label.clone(), Captures::new()),
            }),

            Self::SendType(span, argument, apply) => {
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::SendType(argument.clone(), process),
                })
            }
        })
    }
}

impl Spanning for Apply {
    fn span(&self) -> Span {
        match self {
            Self::Send(span, _, _)
            | Self::Choose(span, _, _)
            | Self::Either(span, _)
            | Self::Begin { span, .. }
            | Self::Loop(span, _)
            | Self::SendType(span, _, _)
            | Self::Noop(span) => *span,
        }
    }
}

impl ApplyBranch {
    pub fn compile(&self) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Then(span, name, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Let {
                    span: span.clone(),
                    name: name.clone(),
                    annotation: None,
                    typ: (),
                    value: Arc::new(process::Expression::Variable(
                        span.clone(),
                        LocalName::object(),
                        (),
                    )),
                    then: Arc::new(process::Process::Do {
                        span: span.clone(),
                        name: LocalName::result(),
                        typ: (),
                        command: process::Command::Link(expression),
                    }),
                })
            }

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile()?;
                pattern.compile_receive(0, span, &LocalName::object(), process)
            }

            Self::Continue(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::Continue(Arc::new(process::Process::Do {
                        span: span.clone(),
                        name: LocalName::result(),
                        typ: (),
                        command: process::Command::Link(expression),
                    })),
                })
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: LocalName::object(),
                    typ: (),
                    command: process::Command::ReceiveType(parameter.clone(), process),
                })
            }
        })
    }
}

impl Spanning for ApplyBranch {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Continue(span, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

impl Process {
    pub fn compile(&self, pass: Pass) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Let {
                span,
                pattern,
                value,
                then,
            } => pattern.compile_let(span, value.compile()?, then.compile(pass)?),

            Self::GlobalCommand(global_name, command) => {
                let span = global_name.span;
                let local_name = LocalName {
                    span,
                    string: format!("{}", global_name),
                };
                Arc::new(process::Process::Let {
                    span,
                    name: local_name.clone(),
                    annotation: None,
                    typ: (),
                    value: Arc::new(process::Expression::Global(span, global_name.clone(), ())),
                    then: command.compile(&local_name, pass)?,
                })
            }

            Self::Command(name, command) => command.compile(name, pass)?,

            Self::Telltypes(span, process) => Arc::new(process::Process::Telltypes(
                span.clone(),
                process.compile(pass)?,
            )),

            Self::Noop(span) => match pass {
                Some(process) => process,
                None => Err(CompileError::MustEndProcess(*span))?,
            },
        })
    }
}

impl Spanning for Process {
    fn span(&self) -> Span {
        match self {
            Self::Let { span, .. } | Self::Telltypes(span, _) => span.clone(),
            Self::GlobalCommand(_, command) => command.span(),
            Self::Command(_, command) => command.span(),
            Self::Noop(span) => *span,
        }
    }
}

impl Command {
    pub fn compile(
        &self,
        object_name: &LocalName,
        pass: Pass,
    ) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Then(process) => process.compile(pass)?,

            Self::Link(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Send(span, argument, command) => {
                let argument = argument.compile()?;
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Send(argument, process),
                })
            }

            Self::Receive(span, pattern, command) => {
                let process = command.compile(object_name, pass)?;
                pattern.compile_receive(0, span, object_name, process)
            }

            Self::Choose(span, chosen, command) => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Signal(chosen.clone(), process),
                })
            }

            Self::Either(span, CommandBranches(process_branches), optional_process) => {
                let pass = match optional_process {
                    Some(process) => Some(process.compile(pass)?),
                    None => pass,
                };

                let mut branches = Vec::new();
                let mut processes = Vec::new();
                for (branch_name, process_branch) in process_branches {
                    branches.push(branch_name.clone());
                    processes.push(process_branch.compile(object_name, pass.clone())?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Case(branches, processes),
                })
            }

            Self::Break(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: object_name.clone(),
                typ: (),
                command: process::Command::Break,
            }),

            Self::Continue(span, process) => {
                let process = process.compile(pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Continue(process),
                })
            }

            Self::Begin {
                span,
                unfounded,
                label,
                then: command,
            } => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone(),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: object_name.clone(),
                typ: (),
                command: process::Command::Loop(label.clone(), Captures::new()),
            }),

            Self::SendType(span, argument, command) => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::SendType(argument.clone(), process),
                })
            }

            Self::ReceiveType(span, parameter, command) => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::ReceiveType(parameter.clone(), process),
                })
            }
        })
    }
}

impl Spanning for Command {
    fn span(&self) -> Span {
        match self {
            Self::Link(span, _)
            | Self::Send(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Choose(span, _, _)
            | Self::Either(span, _, _)
            | Self::Break(span)
            | Self::Continue(span, _)
            | Self::Begin { span, .. }
            | Self::Loop(span, _)
            | Self::SendType(span, _, _)
            | Self::ReceiveType(span, _, _) => span.clone(),

            Self::Then(process) => process.span(),
        }
    }
}

impl CommandBranch {
    pub fn compile(
        &self,
        object_name: &LocalName,
        pass: Pass,
    ) -> Result<Arc<process::Process<()>>, CompileError> {
        Ok(match self {
            Self::Then(_, process) => process.compile(pass)?,

            Self::BindThen(span, name, process) => {
                let process = process.compile(pass)?;
                Arc::new(process::Process::Let {
                    span: span.clone(),
                    name: name.clone(),
                    annotation: None,
                    typ: (),
                    value: Arc::new(process::Expression::Variable(
                        span.clone(),
                        object_name.clone(),
                        (),
                    )),
                    then: process,
                })
            }

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile(object_name, pass)?;
                pattern.compile_receive(0, span, object_name, process)
            }

            Self::Continue(span, process) => {
                let process = process.compile(pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::Continue(process),
                })
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_name.clone(),
                    typ: (),
                    command: process::Command::ReceiveType(parameter.clone(), process),
                })
            }
        })
    }
}

impl Spanning for CommandBranch {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _)
            | Self::BindThen(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Continue(span, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}
