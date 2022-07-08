/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Truncate `val` to the most significant `digits` decimal digits.
pub fn truncate_int(val: u64, digits: u32) -> u64 {
    let mut factor = 1;
    while val > factor {
        factor *= 10;
    }
    factor = (factor / 10_u64.pow(digits)).max(1);
    val / factor * factor
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_truncate_int() {
        assert_eq!(truncate_int(0, 0), 0);
        assert_eq!(truncate_int(1, 1), 1);
        assert_eq!(truncate_int(11, 1), 10);
        assert_eq!(truncate_int(19, 1), 10);
        assert_eq!(truncate_int(123456, 1), 100000);
        assert_eq!(truncate_int(123456, 3), 123000);
        assert_eq!(truncate_int(123456, 7), 123456);
    }
}
