use crate::{
    icombs::readback::TypedHandle,
    location::FileName,
    par::{
        builtin::import_builtins,
        program::{CheckedModule, Definition, Module, ParseAndCompileError, TypeOnHover},
    },
    spawn::TokioSpawn,
};
use crate::{
    icombs::{self, IcCompiled},
    par::{language::CompileError, parse::SyntaxError, process::Expression, types::TypeError},
};

use std::fmt::Write;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum Error {
    Syntax(SyntaxError),
    Compile(CompileError),
    InetCompile(crate::icombs::compiler::Error),
    Type(TypeError),
}

impl Error {
    pub fn display(&self, code: Arc<str>) -> String {
        match self {
            Self::Syntax(error) => {
                // Show syntax error with miette's formatting
                format!(
                    "{:?}",
                    miette::Report::from(error.to_owned()).with_source_code(code)
                )
            }

            Self::Compile(error) => format!("{:?}", error.to_report(code)),

            Self::Type(error) => format!("{:?}", error.to_report(code)),

            Self::InetCompile(err) => {
                format!("inet compilation error: {:?}", err)
            }
        }
    }
}

#[derive(Clone)]
pub enum BuildResult {
    None,
    SyntaxError {
        error: SyntaxError,
    },
    CompileError {
        error: CompileError,
    },
    TypeError {
        pretty: String,
        error: TypeError,
    },
    InetError {
        pretty: String,
        checked: Arc<CheckedModule>,
        type_on_hover: TypeOnHover,
        error: icombs::compiler::Error,
    },
    Ok {
        pretty: String,
        checked: Arc<CheckedModule>,
        type_on_hover: TypeOnHover,
        ic_compiled: IcCompiled,
    },
}

impl BuildResult {
    pub fn error(&self) -> Option<Error> {
        match self {
            Self::None => None,
            Self::SyntaxError { error } => Some(Error::Syntax(error.clone())),
            Self::CompileError { error } => Some(Error::Compile(error.clone())),
            Self::TypeError { error, .. } => Some(Error::Type(error.clone())),
            Self::InetError { error, .. } => Some(Error::InetCompile(error.clone())),
            Self::Ok { .. } => None,
        }
    }

    pub fn pretty(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::SyntaxError { .. } => None,
            Self::CompileError { .. } => None,
            Self::TypeError { pretty, .. } => Some(&pretty),
            Self::InetError { pretty, .. } => Some(&pretty),
            Self::Ok { pretty, .. } => Some(&pretty),
        }
    }

    pub fn checked(&self) -> Option<Arc<CheckedModule>> {
        match self {
            Self::None => None,
            Self::SyntaxError { .. } => None,
            Self::CompileError { .. } => None,
            Self::TypeError { .. } => None,
            Self::InetError { checked, .. } => Some(Arc::clone(checked)),
            Self::Ok { checked, .. } => Some(Arc::clone(checked)),
        }
    }

    pub fn type_on_hover(&self) -> Option<&TypeOnHover> {
        match self {
            Self::None => None,
            Self::SyntaxError { .. } => None,
            Self::CompileError { .. } => None,
            Self::TypeError { .. } => None,
            Self::InetError { type_on_hover, .. } => Some(&type_on_hover),
            Self::Ok { type_on_hover, .. } => Some(&type_on_hover),
        }
    }

    pub fn ic_compiled(&self) -> Option<&IcCompiled> {
        match self {
            Self::None => None,
            Self::SyntaxError { .. } => None,
            Self::CompileError { .. } => None,
            Self::TypeError { .. } => None,
            Self::InetError { .. } => None,
            Self::Ok { ic_compiled, .. } => Some(&ic_compiled),
        }
    }

    pub fn from_source(source: &str, file: FileName) -> Self {
        let mut module = match Module::parse_and_compile(source, file) {
            Ok(module) => module,
            Err(ParseAndCompileError::Parse(error)) => return Self::SyntaxError { error },
            Err(ParseAndCompileError::Compile(error)) => return Self::CompileError { error },
        };
        import_builtins(&mut module);
        Self::from_compiled(module)
    }

    pub fn from_compiled(compiled: Module<Arc<Expression<()>>>) -> Self {
        let pretty = compiled
            .definitions
            .iter()
            .map(
                |Definition {
                     span: _,
                     name,
                     expression,
                 }| {
                    let mut buf = String::new();
                    write!(&mut buf, "def {} = ", name).expect("write failed");
                    expression.pretty(&mut buf, 0).expect("write failed");
                    write!(&mut buf, "\n\n").expect("write failed");
                    buf
                },
            )
            .collect();

        let checked = match compiled.type_check() {
            Ok(checked) => Arc::new(checked),
            Err(error) => return Self::TypeError { pretty, error },
        };
        Self::from_checked(pretty, checked)
    }

    pub fn from_checked(pretty: String, checked: Arc<CheckedModule>) -> Self {
        let type_on_hover = TypeOnHover::new(&checked);
        let ic_compiled = match icombs::compiler::compile_file(&checked) {
            Ok(ic_compiled) => ic_compiled,
            Err(error) => {
                return Self::InetError {
                    pretty,
                    checked,
                    type_on_hover,
                    error,
                }
            }
        };
        Self::Ok {
            pretty,
            checked,
            type_on_hover,
            ic_compiled,
        }
    }
}
