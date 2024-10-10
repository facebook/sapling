/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::ensure;
use anyhow::Context as _;
use anyhow::Result;

/// Wrap `raw_text` in Git SHA1 format so the returned bytes have the SHA1 that
/// matches the Git object identity.
///
/// kind is "commit", "tree", or "blob".
pub fn git_sha1_serialize(raw_text: &[u8], kind: &str) -> Vec<u8> {
    let size_str = raw_text.len().to_string();
    let mut result = Vec::with_capacity(kind.len() + raw_text.len() + size_str.len() + 2);
    result.extend_from_slice(kind.as_bytes());
    result.push(b' ');
    result.extend_from_slice(size_str.as_bytes());
    result.push(0);
    result.extend_from_slice(raw_text);
    result
}

/// The reverse of `git_sha1_serialize`.
/// Take `serialized` and return `raw_text` and `kind`.
pub fn git_sha1_deserialize<'a>(serialized: &'a [u8]) -> Result<(&'a [u8], &'a [u8])> {
    let (kind, rest) =
        split_once(serialized, b' ').context("invalid git object - no space separator")?;
    let (size_str, raw_text) =
        split_once(rest, 0).context("invalid git object - no NUL separator")?;
    let size: usize = std::str::from_utf8(size_str)?.parse()?;
    ensure!(size == raw_text.len(), "invalid git object - wrong size");
    Ok((raw_text, kind))
}

// slice::split_once is not yet stable
fn split_once(data: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    let index = data.iter().position(|&b| b == sep)?;
    Some((&data[..index], &data[index + 1..]))
}
