/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use quickcheck::quickcheck;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug)]
struct Wrap(f64, i32, u8);

// workaround for f64 not implementing Eq
impl PartialEq for Wrap {
    fn eq(&self, other: &Self) -> bool {
        let (f1, f2) = (self.0, other.0);
        (self.1, self.2) == (other.1, other.2) && ((f1.is_nan() && f2.is_nan()) || f1 == f2)
    }
}
impl Eq for Wrap {}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Foo {
    bar: String,
    baz: Option<Wrap>,
    derp: bool,
    list: Vec<u32>,
}

quickcheck! {
    fn test_roundtrip(bar: String, baz: Option<(f64, i32, u8)>, derp: bool, list: Vec<u32>) -> bool {
        let foo = Foo { bar, baz: baz.map(|(a, b, c)| Wrap(a, b, c)), derp, list };
        let bytes = crate::serialize(&foo).unwrap();
        let foo_deserialized: Foo = crate::deserialize(&bytes).unwrap();
        foo == foo_deserialized
    }
}

#[test]
fn test_truncated_slice() {
    // Declares a 5 byte string, but only 2 bytes follow.
    let bytes = [5u8, b'h', b'i'];
    assert!(crate::deserialize::<String>(&bytes).is_err());
    assert!(crate::deserialize::<&[u8]>(&bytes).is_err());
}

#[test]
fn test_slice_length_overflow() {
    // VLQ decoding saturates at u64::MAX, which is usize::MAX on 64-bit.
    let mut bytes = crate::serialize(&u64::MAX).unwrap();
    bytes.extend_from_slice(b"hi");
    assert!(crate::deserialize::<String>(&bytes).is_err());
}

#[test]
fn test_truncated_char() {
    assert!(crate::deserialize::<char>(&[]).is_err());
    // 0xf0 starts a 4 byte UTF-8 sequence, but only 2 bytes are present.
    assert!(crate::deserialize::<char>(&[0xf0, 0x9f]).is_err());
}
