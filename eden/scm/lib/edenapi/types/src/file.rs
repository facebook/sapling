/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::FromIterator;

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

use revisionstore_types::Metadata;
use types::{hgid::HgId, key::Key, parents::Parents};

use crate::{is_default, InvalidHgId};

/// Tombstone string that replaces the content of redacted files.
/// TODO(T48685378): Handle redacted content in a less hacky way.
const REDACTED_TOMBSTONE: &str =
    "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oH\
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
}

impl FileError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Bytes {
        use FileError::*;
        match self {
            Redacted(_, data) => data,
            Corrupt(InvalidHgId { data, .. }) => data,
        }
        .clone()
    }
}

/// Structure representing source control file content on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    key: Key,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    data: Bytes,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    parents: Parents,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    metadata: Metadata,
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
    /// if the content is redacted.
    pub fn data(&self) -> Result<Bytes, FileError> {
        use FileError::*;
        self.data_checked().or_else(|e| match e {
            Corrupt(_) => Err(e),
            Redacted(..) => Ok(e.data()),
        })
    }

    /// Get this entry's data after verifying the hgid hash.
    pub fn data_checked(&self) -> Result<Bytes, FileError> {
        // TODO(T48685378): Handle redacted content in a less hacky way.
        if self.data.len() == REDACTED_TOMBSTONE.len() && self.data == REDACTED_TOMBSTONE {
            return Err(FileError::Redacted(self.key.clone(), self.data.clone()));
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
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct FileRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub keys: Vec<Key>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileResponse {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub entries: Vec<FileEntry>,
}

impl FileResponse {
    pub fn new(entries: impl IntoIterator<Item = FileEntry>) -> Self {
        Self::from_iter(entries)
    }
}

impl FromIterator<FileEntry> for FileResponse {
    fn from_iter<I: IntoIterator<Item = FileEntry>>(entries: I) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
}

impl IntoIterator for FileResponse {
    type Item = FileEntry;
    type IntoIter = std::vec::IntoIter<FileEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}
