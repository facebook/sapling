/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use crate::AnyId;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

/// Token metadata for file content token type.
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct FileContentTokenMetadata {
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

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenData {
    pub id: AnyId,
    pub bubble_id: Option<NonZeroU64>,
    pub metadata: Option<UploadTokenMetadata>,
    // TODO: add other data (like expiration time).
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenSignature {
    pub signature: Vec<u8>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadToken {
    pub data: UploadTokenData,
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            data: Arbitrary::arbitrary(g),
            signature: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokenData {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
            bubble_id: Arbitrary::arbitrary(g),
            metadata: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokenSignature {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            signature: Arbitrary::arbitrary(g),
        }
    }
}
