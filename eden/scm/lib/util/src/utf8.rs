/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Write;

/// Convert input bytes to String by escaping invalid UTF-8 as escaped hex bytes
/// (e.g. "\xC3").
pub fn escape_non_utf8(mut input: &[u8]) -> String {
    let mut output = String::new();

    while !input.is_empty() {
        let (valid_len, invalid_len) = match std::str::from_utf8(input) {
            Ok(_) => (input.len(), 0),
            Err(err) => (
                err.valid_up_to(),
                err.error_len().unwrap_or(input.len() - err.valid_up_to()),
            ),
        };

        // input starts with valid_len bytes of utf8 followed by invalid_len
        // bytes of non-utf8 (followed by more bytes that need checking).

        output.push_str(unsafe { std::str::from_utf8_unchecked(&input[..valid_len]) });
        input = &input[valid_len..];

        for b in &input[..invalid_len] {
            write!(output, r"\x{:X}", b).unwrap();
        }
        input = &input[invalid_len..]
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_non_utf8() {
        assert_eq!(escape_non_utf8(b""), "");
        assert_eq!(escape_non_utf8(b"hello"), "hello");

        assert_eq!(escape_non_utf8(b"\xc3"), r"\xC3");
        assert_eq!(escape_non_utf8(b"\xc3A"), r"\xC3A");
        assert_eq!(escape_non_utf8(b"A\xc3"), r"A\xC3");

        let nihao = "你好".as_bytes();
        assert_eq!(escape_non_utf8(nihao), "你好");
        assert_eq!(escape_non_utf8(&[b"\xc3", nihao].concat()), r"\xC3你好");
        assert_eq!(escape_non_utf8(&[nihao, b"\xc3"].concat()), r"你好\xC3");
        assert_eq!(
            escape_non_utf8(&[nihao, b"\xc3", nihao].concat()),
            r"你好\xC3你好"
        );
    }
}
