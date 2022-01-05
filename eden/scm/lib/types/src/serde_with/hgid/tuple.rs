/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::ser::SerializeTuple;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serializer;

use crate::HgId;

/// Serialize `HgId` as a tuple of 20 `u8`s.
pub fn serialize<S>(id: &HgId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let bytes: &[u8] = id.as_ref();
    let mut tuple = serializer.serialize_tuple(bytes.len())?;
    for i in id.as_ref() {
        tuple.serialize_element(&i)?;
    }
    tuple.end()
}

/// Deserialize `HgId` as a tuple of 20 `u8`s.
pub fn deserialize<'de, D>(deserializer: D) -> Result<HgId, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(HgId::from_byte_array)
}
