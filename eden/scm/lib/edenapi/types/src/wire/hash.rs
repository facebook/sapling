/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

macro_rules! wire_hash {
    {
        wire => $wire: ident,
        api  => $api: ident,
        size => $size: literal,
    } => {

        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $wire([u8; $wire::len()]);

        impl $wire {
            pub const fn len() -> usize {
                $size
            }
        }

        impl std::fmt::Debug for $wire {
            fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(fmt, "{}({:?})", stringify!($wire), types::sha::to_hex(&self.0))
            }
        }

        impl ToWire for $api {
            type Wire = $wire;

            fn to_wire(self) -> Self::Wire {
                $wire(self.into())
            }
        }

        impl ToApi for $wire {
            type Api = $api;
            type Error = std::convert::Infallible;

            fn to_api(self) -> Result<Self::Api, Self::Error> {
                Ok($api::from(self.0))
            }
        }

        impl serde::Serialize for $wire {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_bytes(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $wire {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
                let bytes = bytes.as_ref();

                if bytes.len() == Self::len() {
                    let mut ary = [0u8; Self::len()];
                    ary.copy_from_slice(&bytes);
                    Ok($wire(ary))
                } else {
                    Err(<D::Error as serde::de::Error>::custom($crate::wire::TryFromBytesError {
                        expected_len: Self::len(),
                        found_len: bytes.len(),
                    }))
                }
            }
        }

        #[cfg(any(test, feature = "for-tests"))]
        impl quickcheck::Arbitrary for $wire {
            fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                $api::arbitrary(g).to_wire()
            }
        }
    };

    // fallback when not comma terminated
    {
        wire => $name: ident,
        api  => $api: ident,
        size => $size: literal
    } => {wire_hash! {
        wire => $name,
        api  => $api,
        size => $size,
    }}
}
