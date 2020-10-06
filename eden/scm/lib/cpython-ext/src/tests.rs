/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::PyClone;
use cpython::Python;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use serde_cbor::Value;
use std::collections::HashMap;
use std::fmt::Debug;

#[test]
fn test_serde_basic_types() {
    check_serde_round_trip(&42);
    check_serde_round_trip(&Some(true));
    check_serde_round_trip(&false);
    check_serde_round_trip(&"abc".to_string());
    check_serde_round_trip(&b"abc".to_vec());
    check_serde_round_trip(&(1, (), Some(false), 3));
    check_serde_round_trip(&vec![1, 2, 3]);
    check_serde_round_trip(&());
    check_serde_round_trip(&ByteBuf::from(b"abc".to_vec()));
}

#[test]
fn test_serde_basic_structs() {
    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    struct S;

    check_serde_round_trip(&S);

    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    struct A {
        i: i64,
        s: String,
        u: S,
        e: (),
        t: (bool, (u64, f32)),
        v: Vec<Option<bool>>,
        m: HashMap<u64, String>,
        #[serde(with = "serde_bytes")]
        b: Vec<u8>,
    }

    let a = A {
        i: i64::MIN,
        s: "foo".to_string(),
        u: S,
        e: (),
        t: (true, (u64::MAX, -2.0)),
        v: vec![Some(true), None, Some(false)],
        m: example_hashmap(),
        b: b"abcdef".to_vec(),
    };

    check_serde_round_trip(&a);
}

#[test]
fn test_serde_nested_structs() {
    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    struct A(String, usize);

    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    struct B(Option<Box<A>>, Option<Box<A>>);

    let b = B(None, Some(Box::new(A("abc".to_string(), 42))));
    check_serde_round_trip(&b);
}

#[test]
fn test_serde_enums() {
    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    enum E {
        A,
        B(bool, bool),
        C { x: u8, y: Option<i8> },
        D(Option<Box<E>>),
    }

    let values = [
        E::A,
        E::B(true, false),
        E::C { x: 42, y: None },
        E::D(None),
        E::D(Some(Box::new(E::B(false, true)))),
    ];

    for value in &values {
        check_serde_round_trip(value);
    }
}

fn example_hashmap() -> HashMap<u64, String> {
    let mut m = HashMap::new();
    for i in 1..10 {
        m.insert(i, i.to_string());
    }
    m
}

fn check_serde_round_trip<S>(value: &S)
where
    S: Serialize + PartialEq + Debug,
    for<'de> S: Deserialize<'de>,
{
    let gil = Python::acquire_gil();
    let py = gil.python();
    let obj = crate::ser::to_object(py, &value).unwrap();

    let other: S = crate::de::from_object(py, obj.clone_ref(py)).unwrap();
    assert_eq!(value, &other);

    // Try deserializing into a dynamic type.
    // This exercises the `deserialize_any` code path.
    let dynamic_value: Value = crate::de::from_object(py, obj).unwrap();
    let another: S = serde_cbor::value::from_value(dynamic_value).unwrap();
    assert_eq!(value, &another);
}
