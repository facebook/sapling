/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use anyhow::ensure;
use anyhow::Result;
use types::HgId;
use types::Id20;

use crate::ByteCount;
use crate::Sha1Write;

/// Wrap `raw_text` in Hg SHA1 format so the returned bytes have the SHA1 that
/// matches the Hg object identity.
pub fn hg_sha1_serialize(raw_text: &[u8], p1: &Id20, p2: &Id20) -> Vec<u8> {
    let mut byte_count = ByteCount::default();
    hg_sha1_serialize_write(raw_text, p1, p2, &mut byte_count).unwrap();
    let mut result = Vec::with_capacity(byte_count.into());
    hg_sha1_serialize_write(raw_text, p1, p2, &mut result).unwrap();
    result
}

/// Calculate the SHA1 digest.
pub fn hg_sha1_digest(raw_text: &[u8], p1: &Id20, p2: &Id20) -> HgId {
    let mut hasher = Sha1Write::default();
    hg_sha1_serialize_write(raw_text, p1, p2, &mut hasher).unwrap();
    hasher.into()
}

/// A more general purposed `hg_sha1_serialize` to avoid copies.
/// The `write` function can write directly to a file, or update a SHA1 digest.
pub fn hg_sha1_serialize_write(
    raw_text: &[u8],
    p1: &Id20,
    p2: &Id20,
    out: &mut dyn io::Write,
) -> Result<()> {
    if p1 < p2 {
        out.write_all(p1.as_ref())?;
        out.write_all(p2.as_ref())?;
    } else {
        out.write_all(p2.as_ref())?;
        out.write_all(p1.as_ref())?;
    }
    out.write_all(raw_text)?;
    Ok(())
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
