use arcstr::literal;
use num_bigint::BigUint;

use par_core::frontend::{ExternalTypeDef, PrimitiveType, Type};
use par_core::source::Span;
use par_runtime::readback::Handle;
use par_runtime::registry::{DefinitionRef, ExternalDef, PackageRef};

inventory::submit!(ExternalTypeDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Char",
        name: "Char"
    },
    typ: Type::Primitive(Span::None, PrimitiveType::Char)
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Char",
        name: "Code"
    },
    f: |handle| Box::pin(char_code(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Char",
        name: "Is"
    },
    f: |handle| Box::pin(char_is(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Char",
        name: "ToLower"
    },
    f: |handle| Box::pin(char_to_lower(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Char",
        name: "ToUpper"
    },
    f: |handle| Box::pin(char_to_upper(handle)),
});

async fn char_code(mut handle: Handle) {
    let c = handle.receive().char().await;
    handle.provide_nat(BigUint::from(c as u32));
}

async fn char_to_lower(mut handle: Handle) {
    let ch = handle.receive().char().await;
    handle.provide_char(ch.to_lowercase().next().unwrap_or(ch));
}

async fn char_to_upper(mut handle: Handle) {
    let ch = handle.receive().char().await;
    handle.provide_char(ch.to_uppercase().next().unwrap_or(ch));
}

async fn char_is(mut handle: Handle) {
    let ch = handle.receive().char().await;
    let class = CharClass::readback(handle.receive()).await;
    if class.contains(ch) {
        handle.signal(literal!("true"));
    } else {
        handle.signal(literal!("false"));
    }
    handle.break_();
}

#[derive(Debug, Clone)]
pub(super) enum CharClass {
    Any,
    Char(char),
    Whitespace,
    AsciiAny,
    AsciiAlpha,
    AsciiAlphanum,
    AsciiDigit,
}

impl CharClass {
    pub(super) async fn readback(mut handle: Handle) -> Self {
        match handle.case().await.as_str() {
            "any" => {
                handle.continue_();
                Self::Any
            }
            "ascii" => match handle.case().await.as_str() {
                "alpha" => {
                    handle.continue_();
                    Self::AsciiAlpha
                }
                "alphanum" => {
                    handle.continue_();
                    Self::AsciiAlphanum
                }
                "any" => {
                    handle.continue_();
                    Self::AsciiAny
                }
                "digit" => {
                    handle.continue_();
                    Self::AsciiDigit
                }
                _ => unreachable!(),
            },
            "char" => Self::Char(handle.char().await),
            "whitespace" => {
                handle.continue_();
                Self::Whitespace
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn contains(&self, ch: char) -> bool {
        match self {
            Self::Any => true,
            Self::Char(ch1) => ch == *ch1,
            Self::Whitespace => ch.is_whitespace(),
            Self::AsciiAny => ch.is_ascii(),
            Self::AsciiAlpha => ch.is_ascii_alphabetic(),
            Self::AsciiAlphanum => ch.is_ascii_alphanumeric(),
            Self::AsciiDigit => ch.is_ascii_digit(),
        }
    }
}
