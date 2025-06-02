/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use blob::Blob;
use minibytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use revisionstore_types::Metadata;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use thiserror::Error;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::key::Key;
use types::parents::Parents;

use crate::Blake3;
use crate::InvalidHgId;
use crate::ServerError;
use crate::Sha1;
use crate::UploadToken;
use crate::hash::check_hash;

/// Tombstone string that replaces the content of redacted files.
/// TODO(T48685378): Handle redacted content in a less hacky way.
const REDACTED_TOMBSTONE: &str = "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oH\
      AF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNo\
      UI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQ\
      V3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

#[derive(Debug, Error)]
pub enum FileError {
    #[error(transparent)]
    Corrupt(InvalidHgId),
    /// File entry was redacted by the server. The received content
    /// did not validate but matches the known tombstone content for
    /// redacted data.
    #[error("Content for {0} is redacted")]
    Redacted(Key, Bytes),
    #[error("Can't validate filenode hash for {0} because it is an LFS pointer")]
    Lfs(Key, Bytes),
    #[error("Content for {0} is unavailable")]
    MissingContent(Key),
}

impl FileError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Bytes {
        use FileError::*;
        match self {
            Corrupt(InvalidHgId { data, .. }) => data,
            Redacted(_, data) => data,
            Lfs(_, data) => data,
            MissingContent(_) => panic!("no content attribute available for FileEntry"),
        }
        .clone()
    }
}

/// File "aux data", requires an additional mononoke blobstore lookup. See mononoke_types::ContentMetadataV2.
#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FileAuxData {
    #[id(0)]
    pub total_size: u64,
    // #[id(1)] # deprecated
    #[id(2)]
    pub sha1: Sha1,
    // #[id(3)] # deprecated
    #[id(4)]
    pub blake3: Blake3,
    // None 'file_header_metadata' would mean file_header_metadata is not fetched/not known if it is present
    // Empty metadata would be translated into empty blob
    #[id(5)]
    pub file_header_metadata: Option<Bytes>,
}

/// File content
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileContent {
    pub hg_file_blob: Bytes,
    pub metadata: Metadata,
}

impl FileContent {
    /// Get this entry's data. Checks data integrity but allows hash mismatches
    /// if the content is redacted or contains an LFS pointer.
    pub fn data(&self, key: &Key, parents: Parents) -> Result<Bytes, FileError> {
        use FileError::*;
        self.data_checked(key, parents).or_else(|e| match e {
            Corrupt(_) => Err(e),
            Redacted(..) => Ok(e.data()),
            Lfs(..) => Ok(e.data()),
            MissingContent(_) => Err(e),
        })
    }

    /// Get this entry's data after verifying the hgid hash.
    ///
    /// This method will return an error if the computed hash doesn't match
    /// the provided hash, regardless of the reason. Such mismatches are
    /// sometimes expected (e.g., for redacted files or LFS pointers), so most
    /// application logic should call `FileEntry::data` instead, which allows
    /// these exceptions. `FileEntry::data_checked` should only be used when
    /// strict filenode validation is required.
    pub fn data_checked(&self, key: &Key, parents: Parents) -> Result<Bytes, FileError> {
        // TODO(meyer): Clean this up, make LFS Pointers and redaction strongly typed all the way from here through scmstore
        let data = &self.hg_file_blob;

        // TODO(T48685378): Handle redacted content in a less hacky way.
        if data.len() == REDACTED_TOMBSTONE.len() && data == REDACTED_TOMBSTONE {
            return Err(FileError::Redacted(key.clone(), data.clone()));
        }

        // We can't check the hash of an LFS blob since it is computed using the
        // full file content, but the file entry only contains the LFS pointer.
        if self.metadata().is_lfs() {
            return Err(FileError::Lfs(key.clone(), data.clone()));
        }

        check_hash(data, parents, "blob", key.hgid).map_err(FileError::Corrupt)?;

        Ok(data.clone())
    }

    /// Get this entry's data without verifying the hgid hash.
    pub fn data_unchecked(&self) -> &Bytes {
        &self.hg_file_blob
    }

    /// Get this entry's metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileResponse {
    pub key: Key,
    pub result: Result<FileEntry, ServerError>,
}

/// Structure representing source control file content on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FileEntry {
    pub key: Key,
    pub parents: Parents,
    pub content: Option<FileContent>,
    pub aux_data: Option<FileAuxData>,
}

impl FileAuxData {
    /// Calculate `FileAuxData` from file content.
    pub fn from_content(blob: &Blob) -> Self {
        let total_size = blob.len() as _;
        Self {
            total_size,
            sha1: blob.sha1(),
            blake3: blob.blake3(),
            file_header_metadata: None, // can't be calculated
        }
    }
}

impl FileEntry {
    pub fn new(key: Key, parents: Parents) -> Self {
        Self {
            key,
            parents,
            ..Default::default()
        }
    }

    pub fn with_content(mut self, content: FileContent) -> Self {
        self.content = Some(content);
        self
    }

    pub fn with_aux_data(mut self, aux_data: FileAuxData) -> Self {
        self.aux_data = Some(aux_data);
        self
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    pub fn parents(&self) -> &Parents {
        &self.parents
    }

    pub fn aux_data(&self) -> Option<&FileAuxData> {
        self.aux_data.as_ref()
    }

    pub fn content(&self) -> Option<&FileContent> {
        self.content.as_ref()
    }

    pub fn data(&self) -> Result<Bytes, FileError> {
        self.content
            .as_ref()
            .ok_or_else(|| FileError::MissingContent(self.key().clone()))?
            .data(self.key(), *self.parents())
    }

    pub fn metadata(&self) -> Result<&Metadata, FileError> {
        Ok(self
            .content
            .as_ref()
            .ok_or_else(|| FileError::MissingContent(self.key().clone()))?
            .metadata())
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FileAttributes {
    #[id(0)]
    pub content: bool,
    #[id(1)]
    pub aux_data: bool,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FileSpec {
    #[id(0)]
    pub key: Key,
    #[id(1)]
    pub attrs: FileAttributes,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FileRequest {
    // #[id(0)] # deprecated
    #[id(1)]
    pub reqs: Vec<FileSpec>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileContent {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            hg_file_blob: Bytes::from(bytes),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileAuxData {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            total_size: Arbitrary::arbitrary(g),
            sha1: Arbitrary::arbitrary(g),
            blake3: Arbitrary::arbitrary(g),
            file_header_metadata: Some(Bytes::from(bytes)),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct HgFilenodeData {
    #[id(0)]
    pub node_id: HgId,
    #[id(1)]
    pub parents: Parents,
    #[id(2)]
    pub file_content_upload_token: UploadToken,
    #[id(3)]
    pub metadata: Vec<u8>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadHgFilenodeRequest {
    #[id(0)]
    pub data: HgFilenodeData,
}

#[auto_wire]
#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadTokensResponse {
    #[id(2)]
    pub token: UploadToken,
}
