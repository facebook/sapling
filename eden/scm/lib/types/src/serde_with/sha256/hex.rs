/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Serializer;

use crate::Sha256;

/// Serde `serialize_with` function to serialize `Sha256` as hex string.
pub fn serialize<S>(id: &Sha256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex = id.to_hex();
    serializer.serialize_str(&hex)
}

// bytes::deserialize can decode hex.
pub use super::bytes::deserialize;
