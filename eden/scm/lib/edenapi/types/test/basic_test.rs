/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use edenapi_types::ToApi;
use edenapi_types::ToWire;
use edenapi_types::WireToApiConversionError;
use quickcheck_arbitrary_derive::Arbitrary;
use type_macros::auto_wire;

// Simulating edenapi wire crate
pub mod wire {
    pub fn is_default<T: Default + PartialEq>(t: &T) -> bool {
        t == &Default::default()
    }
}

#[auto_wire]
#[derive(
    Arbitrary,
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    Clone,
    PartialEq,
    Eq
)]
struct ApiObj {
    /// Doc comment should work here
    #[id(0)]
    a: i64,
    #[id(1)]
    /// Doc comment should also work here
    b: u8,
}

#[auto_wire]
#[derive(Arbitrary, Default, Debug, Clone, PartialEq, Eq)]
struct ComplexObj {
    #[id(1)]
    inner: ApiObj,
    #[id(2)]
    b: bool,
}

#[auto_wire]
#[derive(Arbitrary, Clone, Debug, PartialEq, Eq)]
enum MyEnum {
    #[id(1)]
    A,
    #[id(2)]
    B(u32),
}

impl Default for MyEnum {
    fn default() -> Self {
        Self::A
    }
}

#[test]
fn main() {
    let x = ApiObj { a: 12, b: 42 };
    let y = WireApiObj { a: 12, b: 42 };
    assert_eq!(x.clone().to_wire(), y);
    assert_eq!(x, y.clone().to_api().unwrap());
    assert_eq!(&serde_json::to_string(&y).unwrap(), r#"{"0":12,"1":42}"#);

    let x = ComplexObj { inner: x, b: true };
    let y = WireComplexObj { inner: y, b: true };
    assert_eq!(x.clone().to_wire(), y);
    assert_eq!(x, y.clone().to_api().unwrap());
    assert_eq!(
        &serde_json::to_string(&y).unwrap(),
        r#"{"1":{"0":12,"1":42},"2":true}"#
    );

    let x = MyEnum::A;
    let y = WireMyEnum::A;
    assert_eq!(x.clone().to_wire(), y);
    assert_eq!(x, y.clone().to_api().unwrap());
    assert_eq!(&serde_json::to_string(&y).unwrap(), r#""1""#);
    assert_eq!(WireMyEnum::default().to_api().unwrap(), MyEnum::A);

    let x = MyEnum::B(12);
    let y = WireMyEnum::B(12);
    assert_eq!(x.clone().to_wire(), y);
    assert_eq!(x, y.clone().to_api().unwrap());
    assert_eq!(&serde_json::to_string(&y).unwrap(), r#"{"2":12}"#);
}
