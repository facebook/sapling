/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities used by the main path history logic.

/// Pick a `mid` value that is between `left` and `right`, but equals to
/// none of them. Try to keep `right - mid` a "fixed" value to increase
/// cache hits (assuming `right` is "root" of a complete dag).
/// Return `None` if there is no such `mid`.
pub(crate) fn pick_mid(left: u64, right: u64) -> Option<u64> {
    if left + 1 >= right {
        return None;
    }
    // Only keep the higest 2 bits of `delta`. Drop other bits
    // (set them to 0). This makes `right - delta` more stable
    // with different `left`s.
    let delta = (right - left + 1) / 2;
    let mask = 0b11u64 << (62u32.saturating_sub(delta.leading_zeros()));
    let mid = right - (delta & mask);
    debug_assert!(mid > left);
    debug_assert!(mid < right);
    Some(mid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_mid() {
        assert_eq!(pick_mid(1, 1), None);
        assert_eq!(pick_mid(10, 11), None);
        assert_eq!(pick_mid(10, 12), Some(11));
        assert_eq!(pick_mid(10, 13), Some(11));
        assert_eq!(pick_mid(10, 15), Some(12));

        let n = 300;
        for left in 0..=n {
            for right in left..=n {
                match pick_mid(left, right) {
                    None => assert!(right - left <= 1),
                    Some(mid) => {
                        assert!(left < mid);
                        assert!(mid < right);
                    }
                }
            }
        }

        // `mid` remains somewhat stable with different `left`s.
        assert_eq!(pick_mid(10, 20000000), Some(11611392));
        assert_eq!(pick_mid(10000, 20000000), Some(11611392));
    }
}
