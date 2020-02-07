/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Bytes;
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
}
