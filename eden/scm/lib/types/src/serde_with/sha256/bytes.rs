/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserializer;
use serde::Serializer;
use serde::de::Error;

use crate::sha::Sha256;

/// Serde `serialize_with` function to serialize `Sha256` as bytes.
pub fn serialize<S>(id: &Sha256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_bytes(id.as_ref())
}

/// Serde `deserialize_with` function to deserialize `Sha256` as list of u8s,
/// or bytes, or a hex string.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Sha256, D::Error>
where
    D: Deserializer<'de>,
{
    // ByteBuf supports both list and bytes.
    let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
    let bytes = bytes.as_ref();
    // Compatible with hex.
    if bytes.len() == Sha256::hex_len() {
        Sha256::from_hex(bytes).map_err(|e| {
            let msg = format!("invalid Sha256: {} ({:?})", e, bytes);
            D::Error::custom(msg)
        })
    } else {
        Sha256::from_slice(bytes).map_err(|e| {
            let msg = format!("invalid Sha256: {} ({:?})", e, bytes);
            D::Error::custom(msg)
        })
    }
}
