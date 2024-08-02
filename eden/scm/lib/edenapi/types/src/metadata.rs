/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::str::FromStr;

use minibytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
pub use types::Blake3;
pub use types::Sha1;
pub use types::Sha256;

use crate::FileAuxData;
use crate::ServerError;

/// Directory entry metadata
#[auto_wire]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct DirectoryMetadata {
    // Expected to match the hash of the directory's encoded augmented mf.
    #[id(0)]
    pub augmented_manifest_id: Blake3,
    // Expected to match the size of the directory's encoded augmented mf.
    #[id(1)]
    pub augmented_manifest_size: u64,
}

pub type WireTreeAuxData = WireDirectoryMetadata;

/// File entry metadata
#[auto_wire]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FileMetadata {
    // #[id(0)] # deprecated
    #[id(1)] //  deprecated
    #[no_default]
    pub content_id: ContentId,
    // #[id(2)] # deprecated
    #[id(3)]
    #[no_default] // for compatibility
    pub size: u64,
    #[id(4)]
    pub content_sha1: Sha1,
    #[id(5)] // deprecated
    #[no_default]
    pub content_sha256: Sha256,
    #[id(6)]
    pub content_blake3: Blake3,
    // None 'file_header_metadata' would mean file_header_metadata is not fetched/not known if it is present
    // Empty metadata would be translated into empty blob
    #[id(7)]
    pub file_header_metadata: Option<Bytes>,
}

impl From<FileMetadata> for FileAuxData {
    fn from(val: FileMetadata) -> Self {
        FileAuxData {
            total_size: val.size,
            sha1: val.content_sha1,
            blake3: val.content_blake3,
            file_header_metadata: val.file_header_metadata,
        }
    }
}

impl From<FileAuxData> for FileMetadata {
    fn from(aux: FileAuxData) -> Self {
        Self {
            size: aux.total_size,
            content_sha1: aux.sha1,
            content_blake3: aux.blake3,
            file_header_metadata: aux.file_header_metadata,
            ..Default::default()
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileMetadata {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            content_id: Arbitrary::arbitrary(g), // deprecated
            size: Arbitrary::arbitrary(g),
            content_sha1: Arbitrary::arbitrary(g),
            content_sha256: Arbitrary::arbitrary(g), // deprecated
            content_blake3: Arbitrary::arbitrary(g),
            file_header_metadata: Some(Bytes::from(bytes)),
        }
    }
}

blake2_hash!(ContentId);
blake2_hash!(FsnodeId);

#[auto_wire]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum FileType {
    #[id(1)]
    Regular,
    #[id(2)]
    Executable,
    #[id(3)]
    Symlink,
}

impl Default for FileType {
    fn default() -> Self {
        Self::Regular
    }
}

#[auto_wire]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum AnyFileContentId {
    #[id(1)]
    ContentId(ContentId),
    #[id(2)]
    Sha1(Sha1),
    #[id(3)]
    Sha256(Sha256),
    #[id(4)]
    SeededBlake3(Blake3),
}

impl Default for AnyFileContentId {
    fn default() -> Self {
        AnyFileContentId::ContentId(ContentId::default())
    }
}

impl FromStr for AnyFileContentId {
    type Err = ServerError;

    fn from_str(s: &str) -> Result<AnyFileContentId, Self::Err> {
        let v: Vec<&str> = s.split('/').collect();
        if v.len() != 2 {
            return Err(Self::Err::generic(
                "AnyFileContentId parsing failure: format is 'idtype/id'",
            ));
        }
        let idtype = v[0];
        let id = v[1];
        let any_file_content_id = match idtype {
            "content_id" => AnyFileContentId::ContentId(ContentId::from_str(id)?),
            "sha1" => AnyFileContentId::Sha1(Sha1::from_str(id)?),
            "sha256" => AnyFileContentId::Sha256(Sha256::from_str(id)?),
            "seeded_blake3" => AnyFileContentId::SeededBlake3(Blake3::from_str(id)?),
            _ => {
                return Err(Self::Err::generic(
                    "AnyFileContentId parsing failure: supported id types are: 'content_id', 'sha1', 'sha256' and 'seeded_blake3'",
                ));
            }
        };
        Ok(any_file_content_id)
    }
}

impl fmt::Display for AnyFileContentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AnyFileContentId::ContentId(id) => write!(f, "{}", id),
            AnyFileContentId::Sha1(id) => write!(f, "{}", id),
            AnyFileContentId::Sha256(id) => write!(f, "{}", id),
            AnyFileContentId::SeededBlake3(id) => write!(f, "{}", id),
        }
    }
}
