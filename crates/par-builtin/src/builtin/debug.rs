use par_runtime::readback::Handle;
use par_runtime::registry::{DefinitionRef, ExternalDef, PackageRef};

async fn debug_log(mut handle: Handle) {
    let string = handle.receive().string().await;
    eprintln!("{}", string.as_str());
    handle.break_();
}

macro_rules! core_debug_external {
    ($name:literal, $f:path $(, $arg:expr)*) => {
        inventory::submit!(ExternalDef {
            path: DefinitionRef {
                package: PackageRef::CORE,
                path: &[],
                module: "Debug",
                name: $name,
            },
            f: |handle| Box::pin($f(handle $(, $arg)*)),
        });
    };
}

core_debug_external!("Log", debug_log);
