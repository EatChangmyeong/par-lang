use crate::frontend::Type;
use crate::frontend::language::Unresolved;
use par_runtime::registry::{DefinitionRef, PackageRef};
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone)]
pub struct ExternalTypeDef {
    pub path: DefinitionRef<'static>,
    pub typ: Type<Unresolved>,
}

inventory::collect!(ExternalTypeDef);

type Registry = HashMap<PackageRef<'static>, Vec<&'static ExternalTypeDef>>;

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let mut map: Registry = HashMap::new();
    for def in inventory::iter::<ExternalTypeDef> {
        map.entry(def.path.package).or_default().push(def);
    }
    map
});

pub fn get_external_type_defs(
    package: PackageRef,
) -> impl Iterator<Item = &'static ExternalTypeDef> {
    REGISTRY.get(&package).into_iter().flatten().copied()
}
