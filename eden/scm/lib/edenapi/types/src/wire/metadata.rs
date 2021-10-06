/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::{
    AnyFileContentId, ContentId, FileType, FsnodeId, Sha1, Sha256, ToApi, ToWire,
    WireToApiConversionError,
};

pub use crate::metadata::{
    WireDirectoryMetadata, WireDirectoryMetadataRequest, WireFileMetadata, WireFileMetadataRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFileType {
    #[serde(rename = "1")]
    Regular,

    #[serde(rename = "2")]
    Executable,

    #[serde(rename = "3")]
    Symlink,

    #[serde(other, rename = "0")]
    Unknown,
}

impl ToWire for FileType {
    type Wire = WireFileType;

    fn to_wire(self) -> Self::Wire {
        use FileType::*;
        match self {
            Regular => WireFileType::Regular,
            Executable => WireFileType::Executable,
            Symlink => WireFileType::Symlink,
        }
    }
}

impl ToApi for WireFileType {
    type Api = FileType;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireFileType::*;
        Ok(match self {
            Unknown => {
                return Err(WireToApiConversionError::UnrecognizedEnumVariant(
                    "WireFileType",
                ));
            }
            Regular => FileType::Regular,
            Executable => FileType::Executable,
            Symlink => FileType::Symlink,
        })
    }
}

wire_hash! {
    wire => WireFsnodeId,
    api  => FsnodeId,
    size => 32,
}

wire_hash! {
    wire => WireContentId,
    api  => ContentId,
    size => 32,
}

wire_hash! {
    wire => WireSha1,
    api  => Sha1,
    size => 20,
}

wire_hash! {
    wire => WireSha256,
    api  => Sha256,
    size => 32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum WireAnyFileContentId {
    #[serde(rename = "1")]
    WireContentId(WireContentId),

    #[serde(rename = "2")]
    WireSha1(WireSha1),

    #[serde(rename = "3")]
    WireSha256(WireSha256),

    #[serde(other, rename = "0")]
    Unknown,
}

impl Default for WireAnyFileContentId {
    fn default() -> Self {
        WireAnyFileContentId::WireContentId(WireContentId::default())
    }
}

impl ToWire for AnyFileContentId {
    type Wire = WireAnyFileContentId;

    fn to_wire(self) -> Self::Wire {
        use AnyFileContentId::*;
        match self {
            ContentId(id) => WireAnyFileContentId::WireContentId(id.to_wire()),
            Sha1(id) => WireAnyFileContentId::WireSha1(id.to_wire()),
            Sha256(id) => WireAnyFileContentId::WireSha256(id.to_wire()),
        }
    }
}

impl ToApi for WireAnyFileContentId {
    type Api = AnyFileContentId;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireAnyFileContentId::*;
        Ok(match self {
            Unknown => {
                return Err(WireToApiConversionError::UnrecognizedEnumVariant(
                    "WireAnyFileContentId",
                ));
            }
            WireContentId(id) => AnyFileContentId::ContentId(id.to_api()?),
            WireSha1(id) => AnyFileContentId::Sha1(id.to_api()?),
            WireSha256(id) => AnyFileContentId::Sha256(id.to_api()?),
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFileType {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use WireFileType::*;

        let variant = g.choose(&[0, 1, 2, 3]).unwrap();
        match variant {
            0 => Regular,
            1 => Executable,
            2 => Symlink,
            3 => Unknown,
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireAnyFileContentId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use WireAnyFileContentId::*;

        let variant = g.choose(&[0, 1, 2]).unwrap();
        match variant {
            0 => WireContentId(Arbitrary::arbitrary(g)),
            1 => WireSha1(Arbitrary::arbitrary(g)),
            2 => WireSha256(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireFileMetadata,
        WireFileMetadataRequest,
        WireDirectoryMetadata,
        WireDirectoryMetadataRequest,
        WireAnyFileContentId
    );
}
