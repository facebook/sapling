/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_types::hash::Blake2;

// Definitions for hashes 1111...1111 to ffff...ffff.

pub const ONES: Blake2 = Blake2::from_byte_array([0x11; 32]);
pub const TWOS: Blake2 = Blake2::from_byte_array([0x22; 32]);
pub const THREES: Blake2 = Blake2::from_byte_array([0x33; 32]);
pub const FOURS: Blake2 = Blake2::from_byte_array([0x44; 32]);
pub const FIVES: Blake2 = Blake2::from_byte_array([0x55; 32]);
pub const SIXES: Blake2 = Blake2::from_byte_array([0x66; 32]);
pub const SEVENS: Blake2 = Blake2::from_byte_array([0x77; 32]);
pub const EIGHTS: Blake2 = Blake2::from_byte_array([0x88; 32]);
pub const NINES: Blake2 = Blake2::from_byte_array([0x99; 32]);
pub const AS: Blake2 = Blake2::from_byte_array([0xaa; 32]);
pub const BS: Blake2 = Blake2::from_byte_array([0xbb; 32]);
pub const CS: Blake2 = Blake2::from_byte_array([0xcc; 32]);
pub const DS: Blake2 = Blake2::from_byte_array([0xdd; 32]);
pub const ES: Blake2 = Blake2::from_byte_array([0xee; 32]);
pub const FS: Blake2 = Blake2::from_byte_array([0xff; 32]);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn verify() {
        assert_eq!(
            format!("{}", ONES),
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(
            format!("{}", TWOS),
            "2222222222222222222222222222222222222222222222222222222222222222"
        );
        assert_eq!(
            format!("{}", THREES),
            "3333333333333333333333333333333333333333333333333333333333333333"
        );
        assert_eq!(
            format!("{}", FOURS),
            "4444444444444444444444444444444444444444444444444444444444444444"
        );
        assert_eq!(
            format!("{}", FIVES),
            "5555555555555555555555555555555555555555555555555555555555555555"
        );
        assert_eq!(
            format!("{}", SIXES),
            "6666666666666666666666666666666666666666666666666666666666666666"
        );
        assert_eq!(
            format!("{}", SEVENS),
            "7777777777777777777777777777777777777777777777777777777777777777"
        );
        assert_eq!(
            format!("{}", EIGHTS),
            "8888888888888888888888888888888888888888888888888888888888888888"
        );
        assert_eq!(
            format!("{}", NINES),
            "9999999999999999999999999999999999999999999999999999999999999999"
        );
        assert_eq!(
            format!("{}", AS),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            format!("{}", BS),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            format!("{}", CS),
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(
            format!("{}", DS),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
        );
        assert_eq!(
            format!("{}", ES),
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        );
        assert_eq!(
            format!("{}", FS),
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        );
    }
}
