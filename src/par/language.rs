// why not rename this file to ast.rs?

use std::{fmt::Display, hash::Hash, sync::Arc};

use super::{
    primitive::Primitive,
    process::{self, Captures},
    types::Type,
};
use crate::location::{Point, Span, Spanning};
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub struct Name {
    pub span: Span,
    pub modules: Vec<String>,
    pub primary: String,
}

#[derive(Clone, Debug)]
pub enum Pattern<Name> {
    Name(Span, Name, Option<Type<Name>>),
    Receive(Span, Box<Self>, Box<Self>),
    Continue(Span),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum Expression<Name> {
    Primitive(Span, Primitive),
    Reference(Span, Name),
    Grouped(Span, Box<Self>),
    Let {
        span: Span,
        pattern: Pattern<Name>,
        expression: Box<Self>,
        then: Box<Self>,
    },
    Do {
        span: Span,
        process: Box<Process<Name>>,
        then: Box<Self>,
    },
    Fork {
        span: Span,
        channel: Name,
        annotation: Option<Type<Name>>,
        process: Box<Process<Name>>,
    },
    Construction(Construct<Name>),
    Application(Span, Box<Self>, Apply<Name>),
}

#[derive(Clone, Debug)]
pub enum Construct<Name> {
    /// wraps an expression
    Then(Box<Expression<Name>>),
    Send(Span, Box<Expression<Name>>, Box<Self>),
    Receive(Span, Pattern<Name>, Box<Self>),
    /// constructs an either type
    Choose(Span, Name, Box<Self>),
    /// constructs a choice type
    Either(Span, ConstructBranches<Name>),
    /// ! (unit)
    Break(Span),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<Name>,
        then: Box<Self>,
    },
    Loop(Span, Option<Name>),
    SendType(Span, Type<Name>, Box<Self>),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct ConstructBranches<Name>(pub IndexMap<Name, ConstructBranch<Name>>);

#[derive(Clone, Debug)]
pub enum ConstructBranch<Name> {
    Then(Span, Expression<Name>),
    Receive(Span, Pattern<Name>, Box<Self>),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug)]
pub enum Apply<Name> {
    Noop(Point),
    Send(Span, Box<Expression<Name>>, Box<Self>),
    Choose(Span, Name, Box<Self>),
    Either(Span, ApplyBranches<Name>),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<Name>,
        then: Box<Self>,
    },
    Loop(Span, Option<Name>),
    SendType(Span, Type<Name>, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct ApplyBranches<Name>(pub IndexMap<Name, ApplyBranch<Name>>);

#[derive(Clone, Debug)]
pub enum ApplyBranch<Name> {
    Then(Span, Name, Expression<Name>),
    Receive(Span, Pattern<Name>, Box<Self>),
    Continue(Span, Expression<Name>),
    ReceiveType(Span, Name, Box<Self>),
}

// span doesn't include the "then" process
#[derive(Clone, Debug)]
pub enum Process<Name> {
    Let {
        span: Span,
        pattern: Pattern<Name>,
        value: Box<Expression<Name>>,
        then: Box<Self>,
    },
    Command(Name, Command<Name>),
    Telltypes(Span, Box<Self>),
    Noop(Point),
}

#[derive(Clone, Debug)]
pub enum Command<Name> {
    Then(Box<Process<Name>>),
    Link(Span, Box<Expression<Name>>),
    Send(Span, Expression<Name>, Box<Self>),
    Receive(Span, Pattern<Name>, Box<Self>),
    Choose(Span, Name, Box<Self>),
    Either(Span, CommandBranches<Name>, Option<Box<Process<Name>>>),
    Break(Span),
    Continue(Span, Box<Process<Name>>),
    Begin {
        span: Span,
        unfounded: bool,
        label: Option<Name>,
        then: Box<Self>,
    },
    Loop(Span, Option<Name>),
    SendType(Span, Type<Name>, Box<Self>),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug)]
pub struct CommandBranches<Name>(pub IndexMap<Name, CommandBranch<Name>>);

#[derive(Clone, Debug)]
pub enum CommandBranch<Name> {
    Then(Span, Process<Name>),
    Receive(Span, Pattern<Name>, Box<Self>),
    Continue(Span, Process<Name>),
    ReceiveType(Span, Name, Box<Self>),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Internal<Name> {
    Original(Name),
    Result(Option<Name>),
    Object(Option<Name>),
    Match(usize),
}

impl Hash for Name {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (&self.modules, &self.primary).hash(state);
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        (&self.modules, &self.primary) == (&other.modules, &other.primary)
    }
}

impl Eq for Name {}

impl PartialOrd for Name {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (&self.modules, &self.primary).partial_cmp(&(&other.modules, &other.primary))
    }
}

impl Ord for Name {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.modules, &self.primary).cmp(&(&other.modules, &other.primary))
    }
}

impl Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for module in &self.modules {
            write!(f, "{}.", module)?;
        }
        write!(f, "{}", self.primary)
    }
}

impl Spanning for Internal<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Original(name) | Self::Result(Some(name)) | Self::Object(Some(name)) => {
                name.span.clone()
            }

            _ => Span {
                start: Point {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Point {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        }
    }
}

impl<Name: From<String>> From<String> for Internal<Name> {
    fn from(value: String) -> Self {
        Self::Original(Name::from(value))
    }
}

impl<Name: Display> Display for Internal<Name> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Original(name) => write!(f, "{}", name),
            Self::Result(name) => {
                if let Some(name) = name {
                    write!(f, "{}", name)?;
                }
                write!(f, "#result")
            }
            Self::Object(name) => {
                if let Some(name) = name {
                    write!(f, "{}", name)?;
                }
                write!(f, "#object")
            }
            Self::Match(level) => write!(f, "#match{}", level),
        }
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

type Pass<Name> = Option<Arc<process::Process<Internal<Name>, ()>>>;

impl<Name: Clone + Hash + Eq> Pattern<Name> {
    pub fn compile_let(
        &self,
        span: &Span,
        expression: Arc<process::Expression<Internal<Name>, ()>>,
        process: Arc<process::Process<Internal<Name>, ()>>,
    ) -> Arc<process::Process<Internal<Name>, ()>> {
        if let Self::Name(_, name, annotation) = self {
            return Arc::new(process::Process::Let {
                span: span.clone(),
                name: Internal::Original(name.clone()),
                annotation: original(annotation),
                typ: (),
                value: expression,
                then: process,
            });
        }
        Arc::new(process::Process::Let {
            span: span.clone(),
            name: Internal::Match(0),
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
        subject: &Internal<Name>,
        process: Arc<process::Process<Internal<Name>, ()>>,
    ) -> Arc<process::Process<Internal<Name>, ()>> {
        if let Self::Name(_, name, annotation) = self {
            return Arc::new(process::Process::Do {
                span: span.clone(),
                name: subject.clone(),
                typ: (),
                command: process::Command::Receive(
                    Internal::Original(name.clone()),
                    original(annotation),
                    (),
                    process,
                ),
            });
        }
        Arc::new(process::Process::Do {
            span: span.clone(),
            name: subject.clone(),
            typ: (),
            command: process::Command::Receive(
                Internal::Match(level),
                self.annotation(),
                (),
                self.compile_helper(level, process),
            ),
        })
    }

    fn compile_helper(
        &self,
        level: usize,
        process: Arc<process::Process<Internal<Name>, ()>>,
    ) -> Arc<process::Process<Internal<Name>, ()>> {
        match self {
            Self::Name(span, name, annotation) => Arc::new(process::Process::Let {
                span: span.clone(),
                name: Internal::Original(name.clone()),
                annotation: original(annotation),
                typ: (),
                value: Arc::new(process::Expression::Reference(
                    span.clone(),
                    Internal::Match(level),
                    (),
                )),
                then: process,
            }),

            Self::Receive(span, first, rest) => first.compile_receive(
                level + 1,
                span,
                &Internal::Match(level),
                rest.compile_helper(level, process),
            ),

            Self::Continue(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: Internal::Match(level),
                typ: (),
                command: process::Command::Continue(process),
            }),

            Self::ReceiveType(span, parameter, rest) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: Internal::Match(level),
                typ: (),
                command: process::Command::ReceiveType(
                    Internal::Original(parameter.clone()),
                    rest.compile_helper(level, process),
                ),
            }),
        }
    }

    fn annotation(&self) -> Option<Type<Internal<Name>>> {
        match self {
            Self::Name(_, _, annotation) => original(annotation),
            Self::Receive(span, first, rest) => {
                let first = first.annotation()?;
                let rest = rest.annotation()?;
                Some(Type::Send(span.clone(), Box::new(first), Box::new(rest)))
            }
            Self::Continue(span) => Some(Type::Break(span.clone())),
            Self::ReceiveType(span, parameter, rest) => {
                let rest = rest.annotation()?;
                Some(Type::SendType(
                    span.clone(),
                    Internal::Original(parameter.clone()),
                    Box::new(rest),
                ))
            }
        }
    }
}

impl<Name> Spanning for Pattern<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Name(span, _, _)
            | Self::Continue(span)
            | Self::Receive(span, _, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

impl<Name: Clone + Hash + Eq> Expression<Name> {
    pub fn compile(&self) -> Result<Arc<process::Expression<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Primitive(span, value) => Arc::new(process::Expression::Primitive(
                span.clone(),
                value.clone(),
                (),
            )),

            Self::Reference(span, name) => Arc::new(process::Expression::Reference(
                span.clone(),
                Internal::Original(name.clone()),
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
                    chan_name: Internal::Result(None),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process: pattern.compile_let(
                        span,
                        expression,
                        Arc::new(process::Process::Do {
                            span: span.clone(),
                            name: Internal::Result(None),
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
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Link(expression),
                })))?;
                Arc::new(process::Expression::Fork {
                    span: span.clone(),
                    captures: Captures::new(),
                    chan_name: Internal::Result(None),
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
                chan_name: Internal::Original(channel.clone()),
                chan_annotation: original(annotation),
                chan_type: (),
                expr_type: (),
                process: process.compile(None)?,
            }),

            Self::Construction(construct) => {
                let process = construct.compile()?;
                Arc::new(process::Expression::Fork {
                    span: construct.span().clone(),
                    captures: Captures::new(),
                    chan_name: Internal::Result(None),
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
                    chan_name: Internal::Result(None),
                    chan_annotation: None,
                    chan_type: (),
                    expr_type: (),
                    process: Arc::new(process::Process::Let {
                        span: span.clone(),
                        name: Internal::Object(None),
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

impl<Name> Spanning for Expression<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Primitive(span, _)
            | Self::Reference(span, _)
            | Self::Grouped(span, _)
            | Self::Let { span, .. }
            | Self::Do { span, .. }
            | Self::Fork { span, .. }
            | Self::Application(span, _, _) => span.clone(),

            Self::Construction(construction) => construction.span(),
        }
    }
}

impl<Name: Clone + Hash + Eq> Construct<Name> {
    pub fn compile(&self) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Then(expression) => {
                let span = expression.span().clone();
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span,
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Send(span, argument, construct) => {
                let argument = argument.compile()?;
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Send(argument, process),
                })
            }

            Self::Receive(span, pattern, construct) => {
                let process = construct.compile()?;
                pattern.compile_receive(0, span, &Internal::Result(None), process)
            }

            Self::Choose(span, chosen, construct) => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Choose(Internal::Original(chosen.clone()), process),
                })
            }

            Self::Either(span, ConstructBranches(construct_branches)) => {
                let mut branches = Vec::new();
                let mut processes = Vec::new();
                for (branch_name, construct_branch) in construct_branches {
                    branches.push(Internal::Original(branch_name.clone()));
                    processes.push(construct_branch.compile()?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Match(branches, processes),
                })
            }

            Self::Break(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: Internal::Result(None),
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
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: Some(Internal::Result(label.clone())),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: Internal::Result(None),
                typ: (),
                command: process::Command::Loop(
                    Some(Internal::Result(label.clone())),
                    Captures::new(),
                ),
            }),

            Self::SendType(span, argument, construct) => {
                let argument = argument.clone().map_names(&mut Internal::Original);
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::SendType(argument, process),
                })
            }

            Self::ReceiveType(span, parameter, construct) => {
                let process = construct.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::ReceiveType(
                        Internal::Original(parameter.clone()),
                        process,
                    ),
                })
            }
        })
    }
}

impl<Name> Spanning for Construct<Name> {
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

impl<Name: Clone + Hash + Eq> ConstructBranch<Name> {
    pub fn compile(&self) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Then(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile()?;
                pattern.compile_receive(0, span, &Internal::Result(None), process)
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::ReceiveType(
                        Internal::Original(parameter.clone()),
                        process,
                    ),
                })
            }
        })
    }
}

impl<Name> Spanning for ConstructBranch<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _) | Self::Receive(span, _, _) | Self::ReceiveType(span, _, _) => {
                span.clone()
            }
        }
    }
}

impl<Name: Clone + Hash + Eq> Apply<Name> {
    pub fn compile(&self) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Noop(point) => {
                let span = point.point_span();
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Result(None),
                    typ: (),
                    command: process::Command::Link(Arc::new(process::Expression::Reference(
                        span.clone(),
                        Internal::Object(None),
                        (),
                    ))),
                })
            }

            Self::Send(span, expression, apply) => {
                let expression = expression.compile()?;
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::Send(expression, process),
                })
            }

            Self::Choose(span, chosen, apply) => {
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::Choose(Internal::Original(chosen.clone()), process),
                })
            }

            Self::Either(span, ApplyBranches(expression_branches)) => {
                let mut branches = Vec::new();
                let mut processes = Vec::new();
                for (branch_name, expression_branch) in expression_branches {
                    branches.push(Internal::Original(branch_name.clone()));
                    processes.push(expression_branch.compile()?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::Match(branches, processes),
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
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: Some(Internal::Object(label.clone())),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: Internal::Object(None),
                typ: (),
                command: process::Command::Loop(
                    Some(Internal::Object(label.clone())),
                    Captures::new(),
                ),
            }),

            Self::SendType(span, argument, apply) => {
                let argument = argument.clone().map_names(&mut Internal::Original);
                let process = apply.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::SendType(argument, process),
                })
            }
        })
    }
}

impl Spanning for Apply<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Send(span, _, _)
            | Self::Choose(span, _, _)
            | Self::Either(span, _)
            | Self::Begin { span, .. }
            | Self::Loop(span, _)
            | Self::SendType(span, _, _) => span.clone(),

            Self::Noop(point) => point.point_span(),
        }
    }
}

impl<Name: Clone + Hash + Eq> ApplyBranch<Name> {
    pub fn compile(&self) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Then(span, name, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Let {
                    span: span.clone(),
                    name: Internal::Original(name.clone()),
                    annotation: None,
                    typ: (),
                    value: Arc::new(process::Expression::Reference(
                        span.clone(),
                        Internal::Object(None),
                        (),
                    )),
                    then: Arc::new(process::Process::Do {
                        span: span.clone(),
                        name: Internal::Result(None),
                        typ: (),
                        command: process::Command::Link(expression),
                    }),
                })
            }

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile()?;
                pattern.compile_receive(0, span, &Internal::Object(None), process)
            }

            Self::Continue(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::Continue(Arc::new(process::Process::Do {
                        span: span.clone(),
                        name: Internal::Result(None),
                        typ: (),
                        command: process::Command::Link(expression),
                    })),
                })
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: Internal::Object(None),
                    typ: (),
                    command: process::Command::ReceiveType(
                        Internal::Original(parameter.clone()),
                        process,
                    ),
                })
            }
        })
    }
}

impl<Name> Spanning for ApplyBranch<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _, _)
            | Self::Receive(span, _, _)
            | Self::Continue(span, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

impl<Name: Clone + Hash + Eq> Process<Name> {
    pub fn compile(
        &self,
        pass: Pass<Name>,
    ) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        Ok(match self {
            Self::Let {
                span,
                pattern,
                value,
                then,
            } => pattern.compile_let(span, value.compile()?, then.compile(pass)?),

            Self::Command(name, command) => command.compile(name, pass)?,

            Self::Telltypes(span, process) => Arc::new(process::Process::Telltypes(
                span.clone(),
                process.compile(pass)?,
            )),

            Self::Noop(point) => match pass {
                Some(process) => process,
                None => Err(CompileError::MustEndProcess(point.point_span()))?,
            },
        })
    }
}

impl<Name> Spanning for Process<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Let { span, .. } | Self::Telltypes(span, _) => span.clone(),

            Self::Command(_, command) => command.span(),
            Self::Noop(point) => point.point_span(),
        }
    }
}

impl<Name: Clone + Hash + Eq> Command<Name> {
    pub fn compile(
        &self,
        object_name: &Name,
        pass: Pass<Name>,
    ) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        let object_internal = Internal::Original(object_name.clone());

        Ok(match self {
            Self::Then(process) => process.compile(pass)?,

            Self::Link(span, expression) => {
                let expression = expression.compile()?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::Link(expression),
                })
            }

            Self::Send(span, argument, command) => {
                let argument = argument.compile()?;
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::Send(argument, process),
                })
            }

            Self::Receive(span, pattern, command) => {
                let process = command.compile(object_name, pass)?;
                pattern.compile_receive(0, span, &object_internal, process)
            }

            Self::Choose(span, chosen, command) => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::Choose(Internal::Original(chosen.clone()), process),
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
                    branches.push(Internal::Original(branch_name.clone()));
                    processes.push(process_branch.compile(object_name, pass.clone())?);
                }
                let branches = Arc::from(branches);
                let processes = Box::from(processes);
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::Match(branches, processes),
                })
            }

            Self::Break(span) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: object_internal,
                typ: (),
                command: process::Command::Break,
            }),

            Self::Continue(span, process) => {
                let process = process.compile(pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
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
                    name: object_internal,
                    typ: (),
                    command: process::Command::Begin {
                        unfounded: *unfounded,
                        label: label.clone().map(Internal::Original),
                        captures: Captures::new(),
                        body: process,
                    },
                })
            }

            Self::Loop(span, label) => Arc::new(process::Process::Do {
                span: span.clone(),
                name: object_internal,
                typ: (),
                command: process::Command::Loop(
                    label.clone().map(Internal::Original),
                    Captures::new(),
                ),
            }),

            Self::SendType(span, argument, command) => {
                let argument = argument.clone().map_names(&mut Internal::Original);
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::SendType(argument, process),
                })
            }

            Self::ReceiveType(span, parameter, command) => {
                let process = command.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::ReceiveType(
                        Internal::Original(parameter.clone()),
                        process,
                    ),
                })
            }
        })
    }
}

impl<Name> Spanning for Command<Name> {
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

impl<Name: Clone + Hash + Eq> CommandBranch<Name> {
    pub fn compile(
        &self,
        object_name: &Name,
        pass: Pass<Name>,
    ) -> Result<Arc<process::Process<Internal<Name>, ()>>, CompileError> {
        let object_internal = Internal::Original(object_name.clone());

        Ok(match self {
            Self::Then(_, process) => process.compile(pass)?,

            Self::Receive(span, pattern, branch) => {
                let process = branch.compile(object_name, pass)?;
                pattern.compile_receive(0, span, &object_internal, process)
            }

            Self::Continue(span, process) => {
                let process = process.compile(pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::Continue(process),
                })
            }

            Self::ReceiveType(span, parameter, branch) => {
                let process = branch.compile(object_name, pass)?;
                Arc::new(process::Process::Do {
                    span: span.clone(),
                    name: object_internal,
                    typ: (),
                    command: process::Command::ReceiveType(
                        Internal::Original(parameter.clone()),
                        process,
                    ),
                })
            }
        })
    }
}

impl<Name> Spanning for CommandBranch<Name> {
    fn span(&self) -> Span {
        match self {
            Self::Then(span, _)
            | Self::Receive(span, _, _)
            | Self::Continue(span, _)
            | Self::ReceiveType(span, _, _) => span.clone(),
        }
    }
}

fn original<Name: Clone + Eq + Hash>(
    annotation: &Option<Type<Name>>,
) -> Option<Type<Internal<Name>>> {
    annotation
        .clone()
        .map(|t| t.map_names(&mut Internal::Original))
}
