/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_macros::mononoke;

use super::*;

#[mononoke::test]
fn test_md5_digest_reads_hex_in_either_case_and_shows_uppercase() {
    let upper = Md5Digest::from_hex(b"000102030405060708090A0B0C0D0EFF").unwrap();
    let lower = Md5Digest::from_hex(b"000102030405060708090a0b0c0d0eff").unwrap();
    assert_eq!(upper, lower);
    assert_eq!("000102030405060708090A0B0C0D0EFF", upper.to_string());
    assert_eq!(
        "Md5Digest(000102030405060708090A0B0C0D0EFF)",
        format!("{upper:?}")
    );
}

#[mononoke::test]
fn test_md5_digest_rejects_anything_but_32_hex_digits() {
    for hex in [
        &b""[..],
        b"000102030405060708090A0B0C0D0EF",
        b"000102030405060708090A0B0C0D0EFF0",
        b"G00102030405060708090A0B0C0D0EFF",
        b"+00102030405060708090A0B0C0D0EFF",
        b"\xC3\xA900102030405060708090A0B0C0D0EF",
    ] {
        assert_eq!(None, Md5Digest::from_hex(hex), "{hex:?}");
    }
}
