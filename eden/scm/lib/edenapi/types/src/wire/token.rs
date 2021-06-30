/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use serde_derive::{Deserialize, Serialize};

use crate::token::{
    FileContentTokenMetadata, UploadToken, UploadTokenData, UploadTokenMetadata,
    UploadTokenSignature,
};
use crate::wire::{is_default, ToApi, ToWire, WireAnyId, WireToApiConversionError};

/// Token metadata for file content token type.
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireFileContentTokenMetadata {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub content_size: u64,
}

/// Token metadata. Could be different for different token types.
/// A signed token guarantee the metadata has been verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireUploadTokenMetadata {
    #[serde(rename = "1")]
    WireFileContentTokenMetadata(WireFileContentTokenMetadata),
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadTokenData {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub id: WireAnyId,
    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub metadata: Option<WireUploadTokenMetadata>,
    // other data to be added ...
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadTokenSignature {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub signature: Vec<u8>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadToken {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub data: WireUploadTokenData,
    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub signature: WireUploadTokenSignature,
}

impl ToWire for UploadToken {
    type Wire = WireUploadToken;

    fn to_wire(self) -> Self::Wire {
        WireUploadToken {
            data: self.data.to_wire(),
            signature: self.signature.to_wire(),
        }
    }
}

impl ToApi for WireUploadToken {
    type Api = UploadToken;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadToken {
            data: self.data.to_api()?,
            signature: self.signature.to_api()?,
        })
    }
}

impl ToWire for UploadTokenSignature {
    type Wire = WireUploadTokenSignature;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            signature: self.signature.to_wire(),
        }
    }
}

impl ToApi for WireUploadTokenSignature {
    type Api = UploadTokenSignature;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            signature: self.signature.to_api()?,
        };
        Ok(api)
    }
}

impl ToWire for UploadTokenData {
    type Wire = WireUploadTokenData;

    fn to_wire(self) -> Self::Wire {
        WireUploadTokenData {
            id: self.id.to_wire(),
            metadata: self.metadata.to_wire(),
        }
    }
}

impl ToApi for WireUploadTokenData {
    type Api = UploadTokenData;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadTokenData {
            id: self.id.to_api()?,
            metadata: self.metadata.to_api()?,
        })
    }
}

impl ToWire for UploadTokenMetadata {
    type Wire = WireUploadTokenMetadata;

    fn to_wire(self) -> Self::Wire {
        use UploadTokenMetadata::*;
        match self {
            FileContentTokenMetadata(meta) => {
                WireUploadTokenMetadata::WireFileContentTokenMetadata(meta.to_wire())
            }
        }
    }
}

impl ToApi for WireUploadTokenMetadata {
    type Api = UploadTokenMetadata;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireUploadTokenMetadata::*;
        Ok(match self {
            WireFileContentTokenMetadata(meta) => {
                UploadTokenMetadata::FileContentTokenMetadata(meta.to_api()?)
            }
        })
    }
}

impl ToWire for FileContentTokenMetadata {
    type Wire = WireFileContentTokenMetadata;

    fn to_wire(self) -> Self::Wire {
        WireFileContentTokenMetadata {
            content_size: self.content_size,
        }
    }
}

impl ToApi for WireFileContentTokenMetadata {
    type Api = FileContentTokenMetadata;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileContentTokenMetadata {
            content_size: self.content_size,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireUploadToken {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            data: Arbitrary::arbitrary(g),
            signature: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireUploadTokenData {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
            metadata: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireUploadTokenSignature {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            signature: Arbitrary::arbitrary(g),
        }
    }
}
