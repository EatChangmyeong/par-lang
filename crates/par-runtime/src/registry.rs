use crate::flat::runtime::ExternalFn;
use crate::linker::{Linked, Unlinked};
use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageRef<'a> {
    Builtin(BuiltinPackage),
    Special(&'a str),
    Local(&'a str),
    Remote(&'a str),
}

impl PackageRef<'_> {
    pub const CORE: Self = Self::Builtin(BuiltinPackage::Core);
    pub const BASIC: Self = Self::Builtin(BuiltinPackage::Basic);
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BuiltinPackage {
    Core,
    Basic,
}

impl BuiltinPackage {
    pub const ALL: &[Self] = &[Self::Core, Self::Basic];

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "core" => Some(BuiltinPackage::Core),
            "basic" => Some(BuiltinPackage::Basic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BuiltinPackage::Core => "core",
            BuiltinPackage::Basic => "basic",
        }
    }
}

impl fmt::Display for BuiltinPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct DefinitionRef<'a> {
    pub package: PackageRef<'a>,
    pub path: &'a [&'a str],
    pub module: &'a str,
    pub name: &'a str,
}

#[derive(Clone, Copy)]
pub struct ExternalDef {
    pub path: DefinitionRef<'static>,
    pub f: ExternalFn,
}

inventory::collect!(ExternalDef);

type Registry = HashMap<Unlinked, Linked>;

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    inventory::iter::<ExternalDef>
        .into_iter()
        .map(|&ExternalDef { path, f }| (path.into(), f))
        .collect()
});

pub fn get_external_fn(path: &Unlinked) -> Option<ExternalFn> {
    REGISTRY.get(path).copied()
}
