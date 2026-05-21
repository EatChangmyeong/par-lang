//package: core
use arcstr::literal;
use num_bigint::{BigInt, BigUint};

use num_integer::Integer;
use num_traits::Zero;
use par_core::frontend::{ExternalTypeDef, PrimitiveType, Type};
use par_core::source::Span;
use par_runtime::readback::Handle;
use par_runtime::registry::{DefinitionRef, ExternalDef, PackageRef};

inventory::submit!(ExternalTypeDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Nat"
    },
    typ: Type::Primitive(Span::None, PrimitiveType::Nat)
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Mod"
    },
    f: |handle| Box::pin(nat_mod(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Min"
    },
    f: |handle| Box::pin(nat_min(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Max"
    },
    f: |handle| Box::pin(nat_max(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Clamp"
    },
    f: |handle| Box::pin(nat_clamp(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Repeat"
    },
    f: |handle| Box::pin(nat_repeat(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "RepeatLazy"
    },
    f: |handle| Box::pin(nat_repeat_lazy(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "Range"
    },
    f: |handle| Box::pin(nat_range(handle)),
});

inventory::submit!(ExternalDef {
    path: DefinitionRef {
        package: PackageRef::CORE,
        path: &[],
        module: "Nat",
        name: "FromString"
    },
    f: |handle| Box::pin(nat_from_string(handle)),
});

async fn nat_mod(mut handle: Handle) {
    let x = handle.receive().nat().await;
    let y = handle.receive().nat().await;
    handle.provide_nat(if y.is_zero() { y } else { x % y });
}

async fn nat_min(mut handle: Handle) {
    let x = handle.receive().nat().await;
    let y = handle.receive().nat().await;
    handle.provide_nat(x.min(y));
}

async fn nat_max(mut handle: Handle) {
    let x = handle.receive().nat().await;
    let y = handle.receive().int().await;
    // max of int and nat is always nat, so we can ignore the sign.
    let (_sign, max) = BigInt::from(x).max(y).into_parts();
    handle.provide_nat(max);
}

async fn nat_clamp(mut handle: Handle) {
    let int = handle.receive().int().await;
    let min = handle.receive().nat().await;
    let max = handle.receive().nat().await;
    // int clamped to two nats is always nat, so we can ignore the sign.
    let (_sign, clamped) = int.clamp(min.into(), max.into()).into_parts();
    handle.provide_nat(clamped);
}

async fn nat_repeat(mut handle: Handle) {
    let mut n = handle.receive().nat().await;
    while n > BigUint::ZERO {
        handle.signal(literal!("step"));
        n.dec();
    }
    handle.signal(literal!("end"));
    handle.break_();
}

async fn nat_repeat_lazy(mut handle: Handle) {
    let n = handle.receive().nat().await;
    nat_repeat_lazy_inner(handle, n);
}

fn nat_repeat_lazy_inner(mut handle: Handle, n: BigUint) {
    if n > BigUint::ZERO {
        handle.signal(literal!("step"));
        handle.provide_box(move |mut handle| {
            let n = &n - 1u32;
            async move {
                match handle.case().await.as_str() {
                    "next" => nat_repeat_lazy_inner(handle, n.clone()),
                    _ => unreachable!(),
                }
            }
        });
    } else {
        handle.signal(literal!("end"));
        handle.break_();
    }
}

async fn nat_range(mut handle: Handle) {
    let lo = handle.receive().nat().await;
    let hi = handle.receive().nat().await;

    let mut i = lo;
    while i < hi {
        handle.signal(literal!("item"));
        handle.send().provide_nat(i.clone());
        i += 1u32;
    }
    handle.signal(literal!("end"));
    handle.break_();
}

async fn nat_from_string(mut handle: Handle) {
    let string = handle.receive().string().await;
    match string.as_str().parse::<BigUint>() {
        Ok(num) => {
            handle.signal(literal!("some"));
            handle.provide_nat(num);
        }
        Err(_) => {
            handle.signal(literal!("none"));
            handle.break_();
        }
    };
}
