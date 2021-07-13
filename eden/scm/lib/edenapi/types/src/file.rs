/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

use revisionstore_types::Metadata;
use types::{hgid::HgId, key::Key, parents::Parents};

use crate::{ContentId, InvalidHgId, Sha1, Sha256, UploadToken};

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
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileAuxData {
    pub total_size: u64,
    pub content_id: ContentId,
    pub sha1: Sha1,
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

/// Structure representing source control file content on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            content: Arbitrary::arbitrary(g),
            aux_data: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileAttributes {
    pub content: bool,
    pub aux_data: bool,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileAttributes {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            content: Arbitrary::arbitrary(g),
            aux_data: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileSpec {
    pub key: Key,
    pub attrs: FileAttributes,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileSpec {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            attrs: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileRequest {
    // TODO(meyer): Deprecate keys field
    pub keys: Vec<Key>,
    pub reqs: Vec<FileSpec>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            reqs: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileAuxData {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            hg_file_blob: Bytes::from(bytes),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct HgFilenodeData {
    pub node_id: HgId,
    pub parents: Parents,
    pub file_content_upload_token: UploadToken,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadHgFilenodeRequest {
    pub data: HgFilenodeData,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadHgFilenodeResponse {
    pub index: usize,
    pub token: UploadToken,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadHgFilenodeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            data: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for HgFilenodeData {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            node_id: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            file_content_upload_token: Arbitrary::arbitrary(g),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}
