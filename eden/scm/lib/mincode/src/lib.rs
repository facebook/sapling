/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod de;
mod error;
mod ser;

#[cfg(test)]
mod tests;

use std::io;

use serde::Deserialize;
use serde::Serialize;

use self::de::Deserializer;
pub use self::error::Error;
pub use self::error::Result;
use self::ser::Serializer;

pub fn serialize<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    let mut out = Vec::new();
    serialize_into(&mut out, value)?;
    Ok(out)
}

pub fn serialize_into<W, T: ?Sized>(writer: W, value: &T) -> Result<()>
where
    W: io::Write,
    T: Serialize,
{
    let mut ser = Serializer::new(writer);
    Serialize::serialize(value, &mut ser)
}

pub fn deserialize<'de, T>(bytes: &'de [u8]) -> Result<T>
where
    T: Deserialize<'de>,
{
    let mut de = Deserializer::new(bytes);
    Deserialize::deserialize(&mut de)
}
