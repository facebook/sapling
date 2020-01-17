/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// NULL is exported for convenience.
use mercurial_types::hash::Sha1;
pub use mercurial_types::hash::NULL;

// Definitions for hashes 1111...1111 to ffff...ffff.

pub const ONES: Sha1 = Sha1::from_byte_array([0x11; 20]);
pub const TWOS: Sha1 = Sha1::from_byte_array([0x22; 20]);
pub const THREES: Sha1 = Sha1::from_byte_array([0x33; 20]);
pub const FOURS: Sha1 = Sha1::from_byte_array([0x44; 20]);
pub const FIVES: Sha1 = Sha1::from_byte_array([0x55; 20]);
pub const SIXES: Sha1 = Sha1::from_byte_array([0x66; 20]);
pub const SEVENS: Sha1 = Sha1::from_byte_array([0x77; 20]);
pub const EIGHTS: Sha1 = Sha1::from_byte_array([0x88; 20]);
pub const NINES: Sha1 = Sha1::from_byte_array([0x99; 20]);
pub const AS: Sha1 = Sha1::from_byte_array([0xaa; 20]);
pub const BS: Sha1 = Sha1::from_byte_array([0xbb; 20]);
pub const CS: Sha1 = Sha1::from_byte_array([0xcc; 20]);
pub const DS: Sha1 = Sha1::from_byte_array([0xdd; 20]);
pub const ES: Sha1 = Sha1::from_byte_array([0xee; 20]);
pub const FS: Sha1 = Sha1::from_byte_array([0xff; 20]);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn verify() {
        assert_eq!(
            format!("{}", NULL),
            "0000000000000000000000000000000000000000"
        );
        assert_eq!(
            format!("{}", ONES),
            "1111111111111111111111111111111111111111"
        );
        assert_eq!(
            format!("{}", TWOS),
            "2222222222222222222222222222222222222222"
        );
        assert_eq!(
            format!("{}", THREES),
            "3333333333333333333333333333333333333333"
        );
        assert_eq!(
            format!("{}", FOURS),
            "4444444444444444444444444444444444444444"
        );
        assert_eq!(
            format!("{}", FIVES),
            "5555555555555555555555555555555555555555"
        );
        assert_eq!(
            format!("{}", SIXES),
            "6666666666666666666666666666666666666666"
        );
        assert_eq!(
            format!("{}", SEVENS),
            "7777777777777777777777777777777777777777"
        );
        assert_eq!(
            format!("{}", EIGHTS),
            "8888888888888888888888888888888888888888"
        );
        assert_eq!(
            format!("{}", NINES),
            "9999999999999999999999999999999999999999"
        );
        assert_eq!(
            format!("{}", AS),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            format!("{}", BS),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            format!("{}", CS),
            "cccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(
            format!("{}", DS),
            "dddddddddddddddddddddddddddddddddddddddd"
        );
        assert_eq!(
            format!("{}", ES),
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        );
        assert_eq!(
            format!("{}", FS),
            "ffffffffffffffffffffffffffffffffffffffff"
        );
    }
}
