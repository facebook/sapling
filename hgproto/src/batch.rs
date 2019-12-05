/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Operations on the "batch" command.
//!
//! Based on the Mercurial wire protocol documentation. See
//! https://www.mercurial-scm.org/repo/hg/file/@/mercurial/help/internals/wireprotocol.txt.

use crate::errors::*;
use bytes::Bytes;
use failure_ext::bail;

/// Unescape a batch-escaped argument key or value.
pub fn unescape(bs: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bs.len());
    for (idx, slice) in bs.split(|b| b == &b':').enumerate() {
        if idx > 0 {
            // "::" or ":<end of string>" are both illegal.
            if slice.is_empty() {
                bail!(ErrorKind::BatchInvalid(
                    String::from_utf8_lossy(bs.as_ref()).into_owned(),
                ));
            }
            out.push(match slice[0] {
                b'c' => b':',
                b'o' => b',',
                b's' => b';',
                b'e' => b'=',
                ch => bail!(ErrorKind::BatchEscape(ch)),
            });
            out.extend_from_slice(&slice[1..]);
        } else {
            out.extend_from_slice(slice);
        }
    }
    Ok(out)
}

/// Escape a batch result.
pub fn escape(res: &Bytes) -> Vec<u8> {
    let mut out = Vec::with_capacity(res.len());
    for b in res {
        match b {
            b':' => out.extend_from_slice(b":c"),
            b',' => out.extend_from_slice(b":o"),
            b';' => out.extend_from_slice(b":s"),
            b'=' => out.extend_from_slice(b":e"),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::{quickcheck, TestResult};

    const BAD_BYTES: [u8; 3] = [b',', b';', b'='];
    const BYTES_TO_ESCAPE: [u8; 4] = [b':', b',', b';', b'='];

    quickcheck! {
        fn test_roundtrip(input: Vec<u8>) -> bool {
            let bytes = Bytes::from(input);
            let escaped = escape(&bytes);
            let unescaped = unescape(&escaped).unwrap();
            unescaped == bytes
        }

        fn test_bad_bytes(input: Vec<u8>) -> bool {
            let bytes = Bytes::from(input);
            let escaped = escape(&bytes);
            escaped.iter().all(|&b| !BAD_BYTES.contains(&b))
        }

        fn test_escaped_length(input: Vec<u8>) -> bool {
            // Each character in BAD_BYTES should cause the length to be
            // extended by 1.
            let nbad = input.iter().fold(0, |acc, &b| {
                if BYTES_TO_ESCAPE.contains(&b) {
                    acc + 1
                } else {
                    acc
                }
            });
            let bytes = Bytes::from(input);
            let escaped = escape(&bytes);
            bytes.len() + nbad == escaped.len()
        }

        fn test_reverse_roundtrip(input: Vec<u8>) -> TestResult {
            // In this case we're not guaranteed that the string is well-formed.
            if input.iter().any(|&b| BAD_BYTES.contains(&b)) {
                return TestResult::discard();
            }
            let unescaped = unescape(&input);
            let unescaped_bytes = match unescaped {
                Err(_) => return TestResult::discard(),
                Ok(v) => Bytes::from(v),
            };
            let escaped = escape(&unescaped_bytes);
            TestResult::from_bool(escaped == input)
        }
    }
}
