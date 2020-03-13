/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{Bytes, Text};
use quickcheck::quickcheck;

quickcheck! {
    fn test_shallow_clone(v: Vec<u8>) -> bool {
        let a: Bytes = v.into();
        let b: Bytes = a.clone();
        a == b && a.as_ptr() == b.as_ptr()
    }

    fn test_shallow_slice(v: Vec<u8>) -> bool {
        let a: Bytes = v.into();
        let b: Bytes = a.slice(..a.len() / 2);
        b == &a[..b.len()] && (b.is_empty() || a.as_ptr() == b.as_ptr())
    }

    fn test_range_of_slice(v: Vec<u8>) -> bool {
        let a: Bytes = v.into();
        let range1 = a.len() / 3.. a.len() * 2 / 3;
        let slice = a.slice(range1.clone());
        if slice.is_empty() {
            true
        } else {
            let range2 = a.range_of_slice(&slice).unwrap();
            range1 == range2
        }
    }

    fn test_text_shallow_clone(v: String) -> bool {
        let a: Text = v.into();
        let b: Text = a.clone();
        a == b && a.as_ptr() == b.as_ptr()
    }
}

static SAMPLE_TEXT: &str = "这是测试用的文字";

#[test]
fn test_text_slice_valid() {
    let a: Text = SAMPLE_TEXT.into();
    let b = a.slice(3..12);
    let s: &str = b.as_ref();
    let c = a.slice_to_bytes(s);
    let d = b.slice_to_bytes(s);
    assert_eq!(b.as_ptr(), c.as_ptr());
    assert_eq!(b.as_ptr(), d.as_ptr());
}

#[test]
fn test_text_to_string() {
    let a: Text = SAMPLE_TEXT.into();
    assert_eq!(a.to_string(), SAMPLE_TEXT.to_string());
}

#[test]
#[should_panic]
fn test_text_slice_invalid() {
    let a: Text = SAMPLE_TEXT.into();
    let _b = a.slice(3..11); // invalid utf-8 boundary
}
