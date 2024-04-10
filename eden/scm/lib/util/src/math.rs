/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Truncate `val` to the most significant `bits` binary digits.
pub fn truncate_int(val: u64, bits: u32) -> u64 {
    let highest = 64 - val.leading_zeros();
    if highest <= bits {
        val
    } else {
        let mask = (1u64 << bits) - 1;
        val & mask.overflowing_shl(highest - bits).0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_truncate_int() {
        assert_eq!(truncate_int(0, 0), 0);
        assert_eq!(truncate_int(1, 1), 1);
        assert_eq!(truncate_int(1, 2), 1);
        assert_eq!(truncate_int(1, 64), 1);
        assert_eq!(truncate_int(0b11, 1), 0b10);
        assert_eq!(truncate_int(0b11, 2), 0b11);
        assert_eq!(truncate_int(0b11, 3), 0b11);
        assert_eq!(truncate_int(0b11, 64), 0b11);
        assert_eq!(truncate_int(0b1011, 1), 0b1000);
        assert_eq!(truncate_int(0b1011, 3), 0b1010);
        assert_eq!(truncate_int(0b1011, 7), 0b1011);
        assert_eq!(truncate_int(u64::MAX, 64), u64::MAX);
        assert_eq!(truncate_int(u64::MAX, 63), u64::MAX - 1);
        assert_eq!(truncate_int(u64::MAX, 1), 1u64 << 63);
        assert_eq!(truncate_int(u64::MAX, 0), 0);

        assert_eq!(truncate_int(11, 1), 8);
        assert_eq!(truncate_int(19, 1), 16);
        assert_eq!(truncate_int(123456, 1), 65536);
        assert_eq!(truncate_int(123456, 3), 114688);
        assert_eq!(truncate_int(123456, 7), 122880);
    }
}
