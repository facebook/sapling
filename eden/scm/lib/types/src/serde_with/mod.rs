/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod hgid;
pub mod key;
pub mod nodeinfo;
pub mod sha256;

/// Use `SerdeType` as the type talking to serde to serialize or deserialize
/// `Self`. Practically, this makes the `serde_with` functions work for both
/// `Option<Key>` and `Key`.
pub trait SerdeWith<SerdeType> {
    fn serialize_with<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
    fn deserialize_with<'de, D: ::serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error>
    where
        Self: Sized;
}

#[macro_export]
macro_rules! serde_with_mod {
    ($modname:ident, $serdetype:ty) => {
        pub mod $modname {
            use ::serde::Deserializer;
            use ::serde::Serializer;
            type SerdeType = $serdetype;

            pub fn serialize<S, T>(key: &T, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
                T: super::SerdeWith<SerdeType>,
            {
                key.serialize_with(serializer)
            }

            pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
            where
                D: Deserializer<'de>,
                T: super::SerdeWith<SerdeType>,
            {
                T::deserialize_with(deserializer)
            }
        }
    };
}
