/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::de::Error;
use serde::Deserializer;
use serde::Serializer;

use crate::HgId;

/// Serde `serialize_with` function to serialize `HgId` as bytes.
pub fn serialize<S>(id: &HgId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_bytes(id.as_ref())
}

/// Serde `deserialize_with` function to deserialize `HgId` as list of u8s,
/// or bytes, or a hex string.
pub fn deserialize<'de, D>(deserializer: D) -> Result<HgId, D::Error>
where
    D: Deserializer<'de>,
{
    // ByteBuf supports both list and bytes.
    let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
    let bytes = bytes.as_ref();
    // Compatible with hex.
    if bytes.len() == HgId::hex_len() {
        HgId::from_hex(bytes).map_err(|e| {
            let msg = format!("invalid HgId: {} ({:?})", e, bytes);
            D::Error::custom(msg)
        })
    } else {
        HgId::from_slice(bytes).map_err(|e| {
            let msg = format!("invalid HgId: {} ({:?})", e, bytes);
            D::Error::custom(msg)
        })
    }
}
