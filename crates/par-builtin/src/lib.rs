#![deny(unreachable_pub)]

mod builtin;

pub use builtin::{builtin_packages, inject_builtin_packages};
