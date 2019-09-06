// Copyright (c) Facebook, Inc. and its affiliates.
// Copyright (c) David Tolnay <dtolnay@gmail.com>
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

mod de;
mod error;
mod ser;

#[cfg(test)]
mod tests;

use self::de::Deserializer;
use self::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::io;

pub use self::error::{Error, Result};

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
