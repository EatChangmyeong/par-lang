//package: core
use num_bigint::BigInt;
use par_runtime::primitive::Number;
use par_runtime::readback::Handle;
use par_runtime::registry::{DefinitionRef, ExternalDef, PackageRef};

macro_rules! core_number_external {
    ($name:literal, $f:path $(, $arg:expr)*) => {
        inventory::submit!(ExternalDef {
            path: DefinitionRef {
                package: PackageRef::CORE,
                path: &[],
                module: "Number",
                name: $name,
            },
            f: |handle| Box::pin($f(handle $(, $arg)*)),
        });
    };
}

core_number_external!("Zero_", number_zero);
core_number_external!("Add", number_add);
core_number_external!("Sub", number_sub);
core_number_external!("Mul", number_mul);
core_number_external!("Div", number_div);
core_number_external!("Neg", number_neg);

async fn number_zero(mut handle: Handle) {
    handle.receive().continue_();
    handle.provide_number(&Number::Zero);
}

async fn number_add(mut handle: Handle) {
    let mut pair = handle.receive();
    let left = pair.receive_number().await;
    let right = pair.number().await;

    let result = match (left, right) {
        (Number::Zero, Number::Zero) => Number::Zero,
        (Number::Zero, Number::Int(y)) => Number::Int(y),
        (Number::Int(x), Number::Zero) => Number::Int(x),
        (Number::Int(x), Number::Int(y)) => Number::Int(x + y),
        (Number::Zero, Number::Float(y)) => Number::Float(y),
        (Number::Float(x), Number::Zero) => Number::Float(x),
        (Number::Float(x), Number::Float(y)) => Number::Float(x + y),
        _ => panic!("Invalid generic number combination"),
    };

    handle.provide_number(&result);
}

async fn number_sub(mut handle: Handle) {
    let mut pair = handle.receive();
    let left = pair.receive_number().await;
    let right = pair.number().await;

    let result = match (left, right) {
        (Number::Zero, Number::Zero) => Number::Zero,
        (Number::Zero, Number::Int(y)) => Number::Int(-y),
        (Number::Int(x), Number::Zero) => Number::Int(x),
        (Number::Int(x), Number::Int(y)) => Number::Int(x - y),
        (Number::Zero, Number::Float(y)) => Number::Float(-y),
        (Number::Float(x), Number::Zero) => Number::Float(x),
        (Number::Float(x), Number::Float(y)) => Number::Float(x - y),
        _ => panic!("Invalid generic number combination"),
    };

    handle.provide_number(&result);
}

async fn number_mul(mut handle: Handle) {
    let mut pair = handle.receive();
    let left = pair.receive_number().await;
    let right = pair.number().await;

    let result = match (left, right) {
        (Number::Zero, Number::Zero) => Number::Zero,
        (Number::Zero, Number::Int(_)) | (Number::Int(_), Number::Zero) => {
            Number::Int(BigInt::ZERO)
        }
        (Number::Int(x), Number::Int(y)) => Number::Int(x * y),
        (Number::Zero, Number::Float(_)) | (Number::Float(_), Number::Zero) => Number::Float(0.0),
        (Number::Float(x), Number::Float(y)) => Number::Float(x * y),
        _ => panic!("Invalid generic number combination"),
    };

    handle.provide_number(&result);
}

async fn number_div(mut handle: Handle) {
    let mut pair = handle.receive();
    let left = pair.receive_number().await;
    let right = pair.number().await;

    let result = match (left, right) {
        (Number::Zero, Number::Zero) => Number::Zero,
        (Number::Zero, Number::Int(_)) => Number::Int(BigInt::ZERO),
        (Number::Int(_), Number::Zero) => Number::Int(BigInt::ZERO),
        (Number::Int(x), Number::Int(y)) => {
            if y == BigInt::ZERO {
                Number::Int(BigInt::ZERO)
            } else {
                Number::Int(x / y)
            }
        }
        (Number::Zero, Number::Float(y)) => Number::Float(0.0 / y),
        (Number::Float(x), Number::Zero) => Number::Float(x / 0.0),
        (Number::Float(x), Number::Float(y)) => Number::Float(x / y),
        _ => panic!("Invalid generic number combination"),
    };

    handle.provide_number(&result);
}

async fn number_neg(mut handle: Handle) {
    let value = handle.receive_number().await;

    let result = match value {
        Number::Zero => Number::Zero,
        Number::Int(value) => Number::Int(-value),
        Number::Float(value) => Number::Float(-value),
    };

    handle.provide_number(&result);
}
