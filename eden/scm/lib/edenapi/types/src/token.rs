/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use crate::AnyId;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::{Arbitrary, Gen};
use serde_derive::{Deserialize, Serialize};
use type_macros::auto_wire;

#[auto_wire]
/// Token metadata for file content token type.
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct FileContentTokenMetadata {
    #[id(1)]
    pub content_size: u64,
}

/// Token metadata. Could be different for different token types.
/// A signed token guarantee the metadata has been verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UploadTokenMetadata {
    FileContentTokenMetadata(FileContentTokenMetadata),
}

impl From<FileContentTokenMetadata> for UploadTokenMetadata {
    fn from(fctm: FileContentTokenMetadata) -> Self {
        Self::FileContentTokenMetadata(fctm)
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenData {
    #[id(1)]
    pub id: AnyId,
    #[id(3)]
    pub bubble_id: Option<NonZeroU64>,
    #[id(2)]
    pub metadata: Option<UploadTokenMetadata>,
    // TODO: add other data (like expiration time).
}

#[auto_wire]
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenSignature {
    #[id(1)]
    pub signature: Vec<u8>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadToken {
    #[id(1)]
    pub data: UploadTokenData,
    #[id(2)]
    pub signature: UploadTokenSignature,
}

impl UploadToken {
    pub fn new_fake_token(id: AnyId, bubble_id: Option<NonZeroU64>) -> Self {
        Self {
            data: UploadTokenData {
                id,
                bubble_id,
                metadata: None,
            },
            signature: UploadTokenSignature {
                signature: "faketokensignature".into(),
            },
        }
    }

    pub fn new_fake_token_with_metadata(
        id: AnyId,
        bubble_id: Option<NonZeroU64>,
        metadata: UploadTokenMetadata,
    ) -> Self {
        Self {
            data: UploadTokenData {
                id,
                bubble_id,
                metadata: Some(metadata),
            },
            signature: UploadTokenSignature {
                signature: "faketokensignature".into(),
            },
        }
    }
    // TODO: implement secure signed tokens
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadToken {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            data: Arbitrary::arbitrary(g),
            signature: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokenData {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
            bubble_id: Arbitrary::arbitrary(g),
            metadata: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokenMetadata {
    fn arbitrary(g: &mut Gen) -> Self {
        Self::FileContentTokenMetadata(Arbitrary::arbitrary(g))
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileContentTokenMetadata {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            content_size: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokenSignature {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            signature: Arbitrary::arbitrary(g),
        }
    }
}
