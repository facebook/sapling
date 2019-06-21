// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Display};
use std::str::FromStr;

use abomonation_derive::Abomonation;
use ascii::{AsciiStr, AsciiString};
use asyncmemo;
use failure_ext::bail_err;
use heapsize_derive::HeapSizeOf;
use quickcheck::{empty_shrinker, Arbitrary, Gen};
use serde;

use crate::blob::BlobstoreValue;
use crate::bonsai_changeset::BonsaiChangeset;
use crate::errors::*;
use crate::file_contents::FileContents;
use crate::hash::{Blake2, Context};
use crate::rawbundle2::RawBundle2;
use crate::thrift;

// There is no NULL_HASH for typed hashes. Any places that need a null hash should use an
// Option type, or perhaps a list as desired.

/// An identifier used throughout Mononoke.
pub trait MononokeId: Copy + Send + 'static {
    /// Blobstore value type associated with given MononokeId type
    type Value: BlobstoreValue<Key = Self>;

    /// Return a key suitable for blobstore use.
    fn blobstore_key(&self) -> String;

    /// Return a prefix before hash used in blobstore
    fn blobstore_key_prefix() -> String;

    /// Compute this Id for some data.
    fn from_data<T: AsRef<[u8]>>(data: T) -> Self;
}

/// An identifier for a changeset in Mononoke.
#[derive(
    Clone,
    Copy,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Debug,
    Hash,
    HeapSizeOf,
    Abomonation
)]
pub struct ChangesetId(Blake2);

/// An identifier for file contents in Mononoke.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, HeapSizeOf)]
pub struct ContentId(Blake2);

/// An identifier for raw bundle2 contents in Mononoke
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, HeapSizeOf)]
pub struct RawBundle2Id(Blake2);

/// Implementations of typed hashes.
macro_rules! impl_typed_hash {
    {
        hash_type => $typed: ident,
        value_type => $value_type: ident,
        context_type => $typed_context: ident,
        context_key => $key: expr,
    } => {
        impl $typed {
            pub const fn new(blake2: Blake2) -> Self {
                $typed(blake2)
            }

            // (this is public because downstream code wants to be able to deserialize these nodes)
            pub fn from_thrift(h: thrift::$typed) -> Result<Self> {
                // This assumes that a null hash is never serialized. This should always be the
                // case.
                match h.0 {
                    thrift::IdType::Blake2(blake2) => Ok($typed(Blake2::from_thrift(blake2)?)),
                    thrift::IdType::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                        stringify!($typed).into(),
                        format!("unknown id type field: {}", x)
                    )),
                }
            }

            pub(crate) fn from_byte_array(arr: [u8; 32]) -> Self {
                $typed(Blake2::from_byte_array(arr))
            }

            #[inline]
            pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
                Blake2::from_bytes(bytes).map(Self::new)
            }

            #[inline]
            pub fn from_str(s: &str) -> Result<Self> {
                Blake2::from_str(s).map(Self::new)
            }

            #[inline]
            pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
                Blake2::from_ascii_str(s).map(Self::new)
            }

            pub fn blake2(&self) -> &Blake2 {
                &self.0
            }

            #[inline]
            pub fn to_hex(&self) -> AsciiString {
                self.0.to_hex()
            }

            // (this is public because downstream code wants to be able to serialize these nodes)
            pub fn into_thrift(self) -> thrift::$typed {
                thrift::$typed(thrift::IdType::Blake2(self.0.into_thrift()))
            }
        }

        impl From<Blake2> for $typed {
            fn from(h: Blake2) -> $typed {
                $typed::new(h)
            }
        }

        impl<'a> From<&'a Blake2> for $typed {
            fn from(h: &'a Blake2) -> $typed {
                $typed::new(*h)
            }
        }

        /// Context for incrementally computing a hash.
        #[derive(Clone)]
        pub struct $typed_context(Context);

        impl $typed_context {
            /// Construct a context.
            #[inline]
            pub fn new() -> Self {
                $typed_context(Context::new($key.as_bytes()))
            }

            #[inline]
            pub fn update<T>(&mut self, data: T)
            where
                T: AsRef<[u8]>,
            {
                self.0.update(data)
            }

            #[inline]
            pub fn finish(self) -> $typed {
                $typed(self.0.finish())
            }
        }

        impl asyncmemo::Weight for $typed {
            fn get_weight(&self) -> usize {
                ::std::mem::size_of::<Blake2>()
            }
        }

        impl MononokeId for $typed {
            type Value = $value_type;

            #[inline]
            fn blobstore_key(&self) -> String {
                format!(concat!($key, ".blake2.{}"), self.0)
            }

            #[inline]
            fn blobstore_key_prefix() -> String {
                concat!($key, ".blake2.").to_string()
            }

            fn from_data<T: AsRef<[u8]>>(data: T) -> Self {
                let mut context = $typed_context::new();
                context.update(data);
                context.finish()
            }
        }

        impl AsRef<[u8]> for $typed {
            fn as_ref(&self) -> &[u8] {
                self.0.as_ref()
            }
        }

        impl Display for $typed {
            fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
                self.0.fmt(fmt)
            }
        }

        impl Arbitrary for $typed {
            fn arbitrary<G: Gen>(g: &mut G) -> Self {
                $typed(Blake2::arbitrary(g))
            }

            fn shrink(&self) -> Box<Iterator<Item = Self>> {
                empty_shrinker()
            }
        }

        impl serde::Serialize for $typed {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(self.to_hex().as_str())
            }
        }

    }
}

impl_typed_hash! {
    hash_type => ChangesetId,
    value_type => BonsaiChangeset,
    context_type => ChangesetIdContext,
    context_key => "changeset",
}

impl_typed_hash! {
    hash_type => ContentId,
    value_type => FileContents,
    context_type => ContentIdContext,
    context_key => "content",
}

impl_typed_hash! {
    hash_type => RawBundle2Id,
    value_type => RawBundle2,
    context_type => RawBundle2IdContext,
    context_key => "rawbundle2",
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn changesetid_thrift_roundtrip(h: ChangesetId) -> bool {
            let v = h.into_thrift();
            let sh = ChangesetId::from_thrift(v)
                .expect("converting a valid Thrift structure should always work");
            h == sh
        }

        fn contentid_thrift_roundtrip(h: ContentId) -> bool {
            let v = h.into_thrift();
            let sh = ContentId::from_thrift(v)
                .expect("converting a valid Thrift structure should always work");
            h == sh
        }
    }

    #[test]
    fn blobstore_key() {
        // These IDs are persistent, and this test is really to make sure that they don't change
        // accidentally.
        let id = ChangesetId::new(Blake2::from_byte_array([1; 32]));
        assert_eq!(id.blobstore_key(), format!("changeset.blake2.{}", id));

        let id = ContentId::new(Blake2::from_byte_array([1; 32]));
        assert_eq!(id.blobstore_key(), format!("content.blake2.{}", id));
    }
}
