/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::hash::Blake2;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::Sha256;

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

// Definition for the hash ff...ffee..eee
pub const FS_ES: Blake2 = Blake2::from_byte_array([
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee,
]);

// Definitions for SHA-1 hashes 1111...1111 to ffff...ffff.

pub const ONES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x11; 20]);
pub const TWOS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x22; 20]);
pub const THREES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x33; 20]);
pub const FOURS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x44; 20]);
pub const FIVES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x55; 20]);
pub const SIXES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x66; 20]);
pub const SEVENS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x77; 20]);
pub const EIGHTS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x88; 20]);
pub const NINES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0x99; 20]);
pub const AS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xaa; 20]);
pub const BS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xbb; 20]);
pub const CS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xcc; 20]);
pub const DS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xdd; 20]);
pub const ES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xee; 20]);
pub const FS_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([0xff; 20]);

// Definition for the SHA-1 hashes ff...ffee..eee
pub const FS_ES_GIT_SHA1: GitSha1 = GitSha1::from_byte_array([
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xee, 0xee, 0xee, 0xee, 0xee, 0xee,
    0xee, 0xee, 0xee, 0xee,
]);

// Definitions for SHA-256 hashes 1111...1111 to ffff...ffff.

pub const ONES_SHA256: Sha256 = Sha256::from_byte_array([0x11; 32]);
pub const TWOS_SHA256: Sha256 = Sha256::from_byte_array([0x22; 32]);
pub const THREES_SHA256: Sha256 = Sha256::from_byte_array([0x33; 32]);
pub const FOURS_SHA256: Sha256 = Sha256::from_byte_array([0x44; 32]);
pub const FIVES_SHA256: Sha256 = Sha256::from_byte_array([0x55; 32]);
pub const SIXES_SHA256: Sha256 = Sha256::from_byte_array([0x66; 32]);
pub const SEVENS_SHA256: Sha256 = Sha256::from_byte_array([0x77; 32]);
pub const EIGHTS_SHA256: Sha256 = Sha256::from_byte_array([0x88; 32]);
pub const NINES_SHA256: Sha256 = Sha256::from_byte_array([0x99; 32]);
pub const AS_SHA256: Sha256 = Sha256::from_byte_array([0xaa; 32]);
pub const BS_SHA256: Sha256 = Sha256::from_byte_array([0xbb; 32]);
pub const CS_SHA256: Sha256 = Sha256::from_byte_array([0xcc; 32]);
pub const DS_SHA256: Sha256 = Sha256::from_byte_array([0xdd; 32]);
pub const ES_SHA256: Sha256 = Sha256::from_byte_array([0xee; 32]);
pub const FS_SHA256: Sha256 = Sha256::from_byte_array([0xff; 32]);

#[cfg(test)]
mod test {
    use super::*;
    use mononoke_types::hash::Blake2Prefix;

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

        // Test format for 'Blake2Prefix' type
        assert_eq!(
            format!("{}", Blake2Prefix::from_bytes(&FS.as_ref()[0..16]).unwrap()),
            "ffffffffffffffffffffffffffffffff"
        );
    }
}
