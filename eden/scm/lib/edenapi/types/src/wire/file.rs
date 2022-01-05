/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::file::FileContent;
use crate::file::FileEntry;
pub use crate::file::WireFileAttributes;
pub use crate::file::WireFileAuxData;
pub use crate::file::WireFileRequest;
pub use crate::file::WireFileSpec;
pub use crate::file::WireHgFilenodeData;
pub use crate::file::WireUploadHgFilenodeRequest;
pub use crate::file::WireUploadTokensResponse;
use crate::wire::is_default;
use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireKey;
use crate::wire::WireParents;
use crate::wire::WireRevisionstoreMetadata;
use crate::wire::WireToApiConversionError;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireFileEntry {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    key: WireKey,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    data: Option<Bytes>,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    parents: WireParents,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    metadata: Option<WireRevisionstoreMetadata>,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    aux_data: Option<WireFileAuxData>,
}

impl ToWire for FileEntry {
    type Wire = WireFileEntry;

    fn to_wire(self) -> Self::Wire {
        let (data, metadata) = self
            .content
            .map_or((None, None), |c| (Some(c.hg_file_blob), Some(c.metadata)));
        WireFileEntry {
            key: self.key.to_wire(),
            parents: self.parents.to_wire(),
            data,
            metadata: metadata.to_wire(),
            aux_data: self.aux_data.to_wire(),
        }
    }
}

impl ToApi for WireFileEntry {
    type Api = FileEntry;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let content = if let Some(hg_file_blob) = self.data {
            Some(FileContent {
                hg_file_blob,
                metadata: self
                    .metadata
                    .ok_or(WireToApiConversionError::CannotPopulateRequiredField(
                        "content.metadata",
                    ))?
                    .to_api()?,
            })
        } else {
            None
        };
        Ok(FileEntry {
            key: self.key.to_api()?,
            // if content is present, metadata must be also
            content,

            aux_data: self.aux_data.to_api()?,
            parents: self.parents.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFileEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Option<Vec<u8>> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: bytes.map(Bytes::from),
            parents: Arbitrary::arbitrary(g),
            metadata: Arbitrary::arbitrary(g),
            aux_data: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireFileRequest,
        WireFileEntry,
        WireUploadHgFilenodeRequest,
        WireUploadTokensResponse
    );
}
