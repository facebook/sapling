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
fn string_length_longer_than_input_is_rejected() {
    // VLQ length 5 followed by a single byte: the declared string is longer
    // than the remaining input.
    let bytes = [0x05, b'a'];
    let r: Result<String, _> = crate::deserialize(&bytes);
    assert!(
        r.is_err(),
        "expected an error for an oversized string length, got {:?}",
        r
    );
}

#[test]
fn empty_input_for_char_is_rejected() {
    let r: Result<char, _> = crate::deserialize(&[]);
    assert!(
        r.is_err(),
        "expected an error for an empty char, got {:?}",
        r
    );
}

#[test]
fn truncated_multibyte_char_is_rejected() {
    // 0xC3 is the lead byte of a two-byte UTF-8 sequence; the continuation
    // byte is missing.
    let bytes = [0xC3];
    let r: Result<char, _> = crate::deserialize(&bytes);
    assert!(
        r.is_err(),
        "expected an error for a truncated char, got {:?}",
        r
    );
}
