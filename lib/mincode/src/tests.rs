/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use quickcheck::quickcheck;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Foo {
    bar: String,
    baz: Option<(f64, i32, u8)>,
    derp: bool,
    list: Vec<u32>,
}

quickcheck! {
    fn test_roundtrip(bar: String, baz: Option<(f64, i32, u8)>, derp: bool, list: Vec<u32>) -> bool {
        let foo = Foo { bar, baz, derp, list };
        let bytes = crate::serialize(&foo).unwrap();
        let foo_deserialized: Foo = crate::deserialize(&bytes).unwrap();
        foo == foo_deserialized
    }
}
