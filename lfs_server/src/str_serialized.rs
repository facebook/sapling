// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use serde::{de, ser};
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

struct FromStrVisitor<T> {
    _phantom: PhantomData<T>,
}

impl<'de, T> de::Visitor<'de> for FromStrVisitor<T>
where
    T: FromStr,
    <T as std::str::FromStr>::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        v.parse::<T>().map_err(E::custom)
    }
}

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: de::Deserializer<'de>,
    T: FromStr,
    <T as std::str::FromStr>::Err: fmt::Display,
{
    deserializer.deserialize_any(FromStrVisitor::<T> {
        _phantom: PhantomData,
    })
}

pub fn serialize<T, S>(t: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: ser::Serializer,
    T: ToString,
{
    serializer.serialize_str(&t.to_string())
}
