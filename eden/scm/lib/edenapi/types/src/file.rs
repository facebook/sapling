/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;
use revisionstore_types::Metadata;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use thiserror::Error;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::key::Key;
use types::parents::Parents;

use crate::ContentId;
use crate::InvalidHgId;
use crate::ServerError;
use crate::Sha1;
use crate::Sha256;
use crate::UploadToken;

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

/// File "aux data", requires an additional mononoke blobstore lookup. See mononoke_types::ContentMetadata.
#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FileAuxData {
    #[id(0)]
    pub total_size: u64,
    #[id(1)]
    pub content_id: ContentId,
    #[id(2)]
    pub sha1: Sha1,
    #[id(3)]
    pub sha256: Sha256,
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

        let computed = HgId::from_content(&data, parents);
        if computed != key.hgid {
            let err = InvalidHgId {
                expected: key.hgid,
                computed,
                parents,
                data: data.clone(),
            };

            return Err(FileError::Corrupt(err));
        }

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
pub struct FileEntry {
    pub key: Key,
    pub parents: Parents,
    pub content: Option<FileContent>,
    pub aux_data: Option<FileAuxData>,
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            content: Arbitrary::arbitrary(g),
            aux_data: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileAttributes {
    #[id(0)]
    pub content: bool,
    #[id(1)]
    pub aux_data: bool,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileAttributes {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            content: Arbitrary::arbitrary(g),
            aux_data: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileSpec {
    #[id(0)]
    pub key: Key,
    #[id(1)]
    pub attrs: FileAttributes,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileSpec {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            attrs: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize)]
pub struct FileRequest {
    // TODO(meyer): Deprecate keys field
    #[id(0)]
    pub keys: Vec<Key>,
    #[id(1)]
    pub reqs: Vec<FileSpec>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            reqs: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileAuxData {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            total_size: Arbitrary::arbitrary(g),
            content_id: Arbitrary::arbitrary(g),
            sha1: Arbitrary::arbitrary(g),
            sha256: Arbitrary::arbitrary(g),
        }
    }
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

#[auto_wire]
#[derive(Clone, Default, Debug, Eq, PartialEq)]
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
pub struct UploadHgFilenodeRequest {
    #[id(0)]
    pub data: HgFilenodeData,
}

#[auto_wire]
#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct UploadTokensResponse {
    #[id(2)]
    pub token: UploadToken,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadHgFilenodeRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            data: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for HgFilenodeData {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            node_id: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            file_content_upload_token: Arbitrary::arbitrary(g),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTokensResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            token: Arbitrary::arbitrary(g),
        }
    }
}
