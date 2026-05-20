//package: core
use arcstr::literal;
use num_bigint::{BigInt, BigUint};
use num_traits::Zero;
use par_core::frontend::{ExternalTypeDef, PrimitiveType, Type};
use par_core::source::Span;
use par_runtime::readback::Handle;
use par_runtime::registry::{DefinitionRef, ExternalDef, PackageRef};

inventory::submit!(ExternalTypeDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Int"
    },
    typ: Type::Primitive(Span::None, PrimitiveType::Int)
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Mod"
    },
    f: |handle| Box::pin(int_mod(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Min"
    },
    f: |handle| Box::pin(int_min(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Max"
    },
    f: |handle| Box::pin(int_max(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Abs"
    },
    f: |handle| Box::pin(int_abs(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Clamp"
    },
    f: |handle| Box::pin(int_clamp(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "Range"
    },
    f: |handle| Box::pin(int_range(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Int",
        name: "FromString"
    },
    f: |handle| Box::pin(int_from_string(handle)),
});

async fn int_mod(mut handle: Handle) {
    let x = handle.receive().int().await;
    let y = handle.receive().nat().await;
    let result = if y.is_zero() {
        BigUint::ZERO
    } else {
        let modulus = num_integer::mod_floor(x, BigInt::from(y));
        BigUint::try_from(modulus)
            .expect("y is always positive so the result should always be positive")
    };
    handle.provide_nat(result);
}

async fn int_min(mut handle: Handle) {
    let x = handle.receive().int().await;
    let y = handle.receive().int().await;
    handle.provide_int(x.min(y));
}

async fn int_max(mut handle: Handle) {
    let x = handle.receive().int().await;
    let y = handle.receive().int().await;
    handle.provide_int(x.max(y));
}

async fn int_clamp(mut handle: Handle) {
    let int = handle.receive().int().await;
    let min = handle.receive().int().await;
    let max = handle.receive().int().await;
    handle.provide_int(int.min(max).max(min));
}

async fn int_abs(mut handle: Handle) {
    let int = handle.receive().int().await;
    let (_sign, magnitude) = int.into_parts();
    handle.provide_nat(magnitude);
}

async fn int_range(mut handle: Handle) {
    let lo = handle.receive().int().await;
    let hi = handle.receive().int().await;

    let mut i = lo;
    while i < hi {
        handle.signal(literal!("item"));
        handle.send().provide_int(i.clone());
        i += 1;
    }
    handle.signal(literal!("end"));
    handle.break_();
}

async fn int_from_string(mut handle: Handle) {
    let string = handle.receive().string().await;
    match string.as_str().parse::<BigInt>() {
        Ok(num) => {
            handle.signal(literal!("some"));
            handle.provide_int(num);
        }
        Err(_) => {
            handle.signal(literal!("none"));
            handle.break_();
        }
    };
}
