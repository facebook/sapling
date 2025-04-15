/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Deserializer;
use serde::Serializer;
use serde::ser::SerializeTuple;

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
