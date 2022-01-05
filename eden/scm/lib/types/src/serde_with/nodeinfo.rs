/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::serde_with::SerdeWith;
use crate::serde_with_mod;

macro_rules! nodeinfo_type {
    ($typename:ident, $with_key:tt, $with_hgid:tt) => {
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        pub struct $typename {
            #[serde(with = $with_key)]
            pub parents: [$crate::Key; 2],
            #[serde(with = $with_hgid)]
            pub linknode: $crate::HgId,
        }

        impl From<$crate::NodeInfo> for $typename {
            fn from(this: $crate::NodeInfo) -> Self {
                Self {
                    parents: this.parents,
                    linknode: this.linknode,
                }
            }
        }

        impl Into<$crate::NodeInfo> for $typename {
            fn into(self) -> $crate::NodeInfo {
                $crate::NodeInfo {
                    parents: self.parents,
                    linknode: self.linknode,
                }
            }
        }

        impl SerdeWith<$typename> for $crate::NodeInfo {
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
    };
}

nodeinfo_type!(
    BytesNodeInfo,
    "crate::serde_with::key::bytes",
    "crate::serde_with::hgid::bytes"
);
nodeinfo_type!(
    HexNodeInfo,
    "crate::serde_with::key::hex",
    "crate::serde_with::hgid::hex"
);
nodeinfo_type!(
    TupleNodeInfo,
    "crate::serde_with::key::tuple",
    "crate::serde_with::hgid::tuple"
);

serde_with_mod!(bytes, super::BytesNodeInfo);
serde_with_mod!(hex, super::HexNodeInfo);
serde_with_mod!(tuple, super::TupleNodeInfo);
