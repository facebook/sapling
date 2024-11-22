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
        b == a[..b.len()] && (b.is_empty() || a.as_ptr() == b.as_ptr())
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
    #[cfg(feature = "non-zerocopy-into")]
    assert_eq!(Vec::<u8>::from(c.slice(..)), b"bc");
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

#[test]
fn test_downgrade_upgrade() {
    let v = b"abcd".to_vec();
    let b = Bytes::from(v);

    // `downgrade` -> `upgrade` returns the same buffer.
    // Slicing is ignored. Full range is used.
    let b1: crate::WeakBytes = b.slice(1..=2).downgrade().unwrap();
    let b2 = Bytes::upgrade(&b1).unwrap();
    assert_eq!(b, b2);
    assert_eq!(b.as_ptr(), b2.as_ptr());

    // `upgrade` returns `None` if all strong refs are dropped.
    drop(b2);
    drop(b);
    let b3 = Bytes::upgrade(&b1);
    assert!(b3.is_none());
}

#[test]
fn test_bytes_to_text() {
    let b1 = Bytes::from_static("abcd 文字".as_bytes());
    let t1 = Text::from_utf8_lossy(b1.clone());
    // zero-copy, b1 and t1 share the same buffer.
    assert_eq!(t1.as_ptr(), b1.as_ptr());
    assert_eq!(t1.as_bytes(), b1.as_bytes());

    let b2 = Bytes::from_static(b"\xff\xfe");
    let t2 = Text::from_utf8_lossy(b2.clone());
    // invalid utf-8, cannot zero-copy
    assert_ne!(t2.as_ptr(), b2.as_ptr());
    assert_ne!(t2.as_bytes(), b2.as_bytes());
}
