/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::SerdeWith;
use crate::serde_with_mod;

macro_rules! key_type {
    ($typename:ident, $with:tt) => {
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        pub struct $typename {
            pub path: $crate::path::RepoPathBuf,
            #[serde(alias = "node", rename = "node", with = $with)]
            pub hgid: $crate::HgId,
        }

        impl From<$crate::Key> for $typename {
            fn from(key: $crate::Key) -> Self {
                Self {
                    path: key.path,
                    hgid: key.hgid,
                }
            }
        }

        impl Into<$crate::Key> for $typename {
            fn into(self) -> $crate::Key {
                $crate::Key {
                    path: self.path,
                    hgid: self.hgid,
                }
            }
        }

        impl SerdeWith<$typename> for $crate::Key {
            fn serialize_with<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let value: $typename = self.clone().into();
                serde::Serialize::serialize(&value, serializer)
            }
            fn deserialize_with<'de, D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let value: $typename = ::serde::Deserialize::deserialize(deserializer)?;
                Ok(value.into())
            }
        }

        impl SerdeWith<$typename> for Option<$crate::Key> {
            fn serialize_with<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let value: Option<$typename> = self.clone().map(Into::into);
                serde::Serialize::serialize(&value, serializer)
            }
            fn deserialize_with<'de, D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let value: Option<$typename> = ::serde::Deserialize::deserialize(deserializer)?;
                let value = value.map(Into::into);
                Ok(value)
            }
        }

        impl SerdeWith<$typename> for [$crate::Key; 2] {
            fn serialize_with<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let [k1, k2] = self.clone();
                let value: [$typename; 2] = [k1.into(), k2.into()];
                serde::Serialize::serialize(&value, serializer)
            }
            fn deserialize_with<'de, D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let value: [$typename; 2] = ::serde::Deserialize::deserialize(deserializer)?;
                let [k1, k2] = value;
                let value = [k1.into(), k2.into()];
                Ok(value)
            }
        }
    };
}

key_type!(BytesKey, "crate::serde_with::hgid::bytes");
key_type!(HexKey, "crate::serde_with::hgid::hex");
key_type!(TupleKey, "crate::serde_with::hgid::tuple");

serde_with_mod!(bytes, super::BytesKey);
serde_with_mod!(hex, super::HexKey);
serde_with_mod!(tuple, super::TupleKey);
