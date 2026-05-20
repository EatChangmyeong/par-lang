#![deny(unreachable_pub)]

mod builtin;

pub use builtin::{
    PAR_BUILTIN_URI_SCHEME, builtin_packages, get_builtin_source, inject_builtin_packages,
};
