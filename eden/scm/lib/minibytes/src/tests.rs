/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use quickcheck::quickcheck;

use crate::Bytes;
use crate::Text;

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

#[test]
fn test_downcast_mut() {
    let v = b"abcd".to_vec();
    let mut b = Bytes::from(v);
    assert!(b.downcast_mut::<Vec<u8>>().is_some());
    assert!(b.downcast_mut::<String>().is_none());
    let mut c = b.clone();
    assert!(b.downcast_mut::<Vec<u8>>().is_none());
    assert!(c.downcast_mut::<Vec<u8>>().is_none());
}

#[test]
fn test_into_vec() {
    let v = b"abcd".to_vec();
    let ptr1 = &v[0] as *const u8;
    let b = Bytes::from(v);
    let v = b.into_vec(); // zero-copy
    let ptr2 = &v[0] as *const u8;
    assert_eq!(ptr1, ptr2);

    let b = Bytes::from(v);
    let _c = b.clone();
    let v = b.into_vec(); // not zero-copy because refcount > 1
    let ptr3 = &v[0] as *const u8;
    assert_ne!(ptr1, ptr3);

    let b = Bytes::from(v);
    let c = b.slice(1..3);
    drop(b);
    assert_eq!(c.into_vec(), b"bc");
}

#[test]
fn test_bytes_debug_format() {
    let v = b"printable\t\r\n\'\"\\\x00\x01\x02printable".to_vec();
    let b = Bytes::from(v);
    let escaped = format!("{:?}", b);
    let expected = r#"b"printable\t\r\n\'\"\\\x00\x01\x02printable""#;
    assert_eq!(escaped, expected);
}
