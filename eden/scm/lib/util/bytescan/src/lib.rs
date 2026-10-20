/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! SIMD-accelerated byte scans shared by binary detection and line counting.
//!
//! Prefer these over `<[u8]>::contains` and predicate-based `split().count()`.
//! `<[u8]>::contains` resolves to `core`'s internal memchr, which is a
//! word-at-a-time SWAR loop rather than the vectorized `memchr` crate, and
//! `split(|c| c == &b'\n')` takes a closure so it never specializes at all.

/// Returns true if `data` looks like binary content.
///
/// Sapling, Mercurial and Git all define "binary" as "contains a NUL byte".
/// Git additionally caps the scan at the first 8000 bytes (`buffer_is_binary`);
/// Sapling has no such cap, and this must stay a whole-buffer test.
/// `LfsPointersEntry::is_binary` persists the result on disk and on the wire,
/// and `cmdcat` calls this per chunk of a chained buffer, so a cap added here
/// would disagree with already-stored pointers and would apply per chunk.
pub fn is_binary(data: &[u8]) -> bool {
    memchr::memchr(0, data).is_some()
}

/// Counts newline bytes in `data`.
///
/// This is a raw `b'\n'` count, not a line count: a buffer without a trailing
/// newline has one more line than it has newlines.
pub fn count_newlines(data: &[u8]) -> usize {
    bytecount::count(data, b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary() {
        assert!(!is_binary(b""));
        assert!(!is_binary(b"hello\nworld\n"));
        assert!(is_binary(b"\0"));
        assert!(is_binary(b"hello\0world"));
    }

    #[test]
    fn test_is_binary_past_the_simd_stride() {
        let mut data = vec![b'a'; 10000];
        assert!(!is_binary(&data));
        data[9999] = 0;
        assert!(is_binary(&data));
    }

    #[test]
    fn test_count_newlines() {
        assert_eq!(count_newlines(b""), 0);
        assert_eq!(count_newlines(b"no trailing newline"), 0);
        assert_eq!(count_newlines(b"a\nb\n"), 2);
        assert_eq!(count_newlines(&vec![b'\n'; 10000]), 10000);
    }
}
