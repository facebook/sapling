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

use crate::InvalidHgId;

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
}

impl FileError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Bytes {
        use FileError::*;
        match self {
            Corrupt(InvalidHgId { data, .. }) => data,
            Redacted(_, data) => data,
            Lfs(_, data) => data,
        }
        .clone()
    }
}

/// Structure representing source control file content on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileEntry {
    pub key: Key,
    pub data: Bytes,
    pub parents: Parents,
    pub metadata: Metadata,
}

impl FileEntry {
    pub fn new(key: Key, data: Bytes, parents: Parents, metadata: Metadata) -> Self {
        Self {
            key,
            data,
            parents,
            metadata,
        }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Get this entry's data. Checks data integrity but allows hash mismatches
    /// if the content is redacted or contains an LFS pointer.
    pub fn data(&self) -> Result<Bytes, FileError> {
        use FileError::*;
        self.data_checked().or_else(|e| match e {
            Corrupt(_) => Err(e),
            Redacted(..) => Ok(e.data()),
            Lfs(..) => Ok(e.data()),
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
    pub fn data_checked(&self) -> Result<Bytes, FileError> {
        // TODO(T48685378): Handle redacted content in a less hacky way.
        if self.data.len() == REDACTED_TOMBSTONE.len() && self.data == REDACTED_TOMBSTONE {
            return Err(FileError::Redacted(self.key.clone(), self.data.clone()));
        }

        // We can't check the hash of an LFS blob since it is computed using the
        // full file content, but the file entry only contains the LFS pointer.
        if self.metadata.is_lfs() {
            return Err(FileError::Lfs(self.key.clone(), self.data.clone()));
        }

        let computed = HgId::from_content(&self.data, self.parents);
        if computed != self.key.hgid {
            let err = InvalidHgId {
                expected: self.key.hgid,
                computed,
                data: self.data.clone(),
                parents: self.parents,
            };

            return Err(FileError::Corrupt(err));
        }

        Ok(self.data.clone())
    }

    /// Get this entry's data without verifying the hgid hash.
    pub fn data_unchecked(&self) -> Bytes {
        self.data.clone()
    }

    /// Get this entry's metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn parents(&self) -> &Parents {
        &self.parents
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: Bytes::from(bytes),
            parents: Arbitrary::arbitrary(g),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileRequest {
    pub keys: Vec<Key>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}
