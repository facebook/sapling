/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::ensure;
use anyhow::Result;
use types::Id20;

/// Wrap `raw_text` in Hg SHA1 format so the returned bytes have the SHA1 that
/// matches the Hg object identity.
pub fn hg_sha1_serialize(raw_text: &[u8], p1: &Id20, p2: &Id20) -> Vec<u8> {
    let mut result = Vec::with_capacity(raw_text.len() + Id20::len() * 2);
    if p1 < p2 {
        result.extend_from_slice(p1.as_ref());
        result.extend_from_slice(p2.as_ref());
    } else {
        result.extend_from_slice(p2.as_ref());
        result.extend_from_slice(p1.as_ref());
    }
    result.extend_from_slice(raw_text);
    result
}

/// The reverse of `hg_sha1_serialize`.
/// Take `serialized` and return `raw_text`, `pa`, and `pb`.
///
/// Note: `pa`, `pb` no longer preserve `p1`, `p2` order. `(pa, pb)` could be
/// `(p1, p2)` or `(p2, p1)`.
pub fn hg_sha1_deserialize<'a>(serialized: &'a [u8]) -> Result<(&'a [u8], Id20, Id20)> {
    ensure!(
        serialized.len() >= Id20::len() * 2,
        "invalid hg SHA1 serialized data - insufficient length"
    );
    let pa = Id20::from_slice(&serialized[..Id20::len()]).unwrap();
    let pb = Id20::from_slice(&serialized[Id20::len()..Id20::len() * 2]).unwrap();
    let raw_text = &serialized[Id20::len() * 2..];
    Ok((raw_text, pa, pb))
}
