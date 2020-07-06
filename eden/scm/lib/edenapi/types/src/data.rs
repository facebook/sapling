/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::FromIterator;

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use thiserror::Error;

use types::{hgid::HgId, key::Key, parents::Parents};

/// Tombstone string that replaces the content of redacted files.
/// TODO(T48685378): Handle redacted content in a less hacky way.
const REDACTED_TOMBSTONE: &str =
    "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oH\
     AF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNo\
     UI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQ\
     V3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

#[derive(Debug, Error)]
pub enum DataError {
    #[error(transparent)]
    Corrupt(InvalidHgId),
    /// Data entry was redacted by the server. The received content
    /// did not validate but matches the known tombstone content for
    /// redacted data.
    #[error("Content for {0} is redacted")]
    Redacted(Key, Bytes),
    /// If this entry contains manifest content and represents a root node
    /// (i.e., has an empty path), it may be a hybrid tree manifest which
    /// has the content of a root tree manifest node, but the hash of the
    /// corresponding flat manifest. This situation should only occur for
    /// manifests created in "hybrid mode" (i.e., during a transition from
    /// flat manifests to tree manifests).
    #[error("Detected possible hybrid manifest: {0}")]
    MaybeHybridManifest(#[source] InvalidHgId),
}

impl DataError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Bytes {
        use DataError::*;
        match self {
            Redacted(_, data) => data,
            Corrupt(InvalidHgId { data, .. }) => data,
            MaybeHybridManifest(InvalidHgId { data, .. }) => data,
        }
        .clone()
    }
}

#[derive(Debug, Error)]
#[error("Invalid hash: {expected} (expected) != {computed} (computed)")]
pub struct InvalidHgId {
    expected: HgId,
    computed: HgId,
    data: Bytes,
    parents: Parents,
}

/// Structure representing source control data (typically either file content
/// or a tree entry) on the wire. Includes the information required to add the
/// data to a mutable store, along with the parents for hash validation.
#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct DataEntry {
    key: Key,
    data: Bytes,
    parents: Parents,
}

impl DataEntry {
    pub fn new(key: Key, data: Bytes, parents: Parents) -> Self {
        Self { key, data, parents }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Get this entry's data. Checks data integrity but allows hash mismatches
    /// if the content is redacted or if this is a suspected hybrid manifest.
    pub fn data(&self) -> Result<Bytes, DataError> {
        use DataError::*;
        self.data_checked().or_else(|e| match e {
            Corrupt(_) => Err(e),
            Redacted(..) | MaybeHybridManifest(_) => Ok(e.data()),
        })
    }

    /// Get this entry's data after verifying the hgid hash.
    pub fn data_checked(&self) -> Result<Bytes, DataError> {
        // TODO(T48685378): Handle redacted content in a less hacky way.
        if self.data.len() == REDACTED_TOMBSTONE.len() && self.data == REDACTED_TOMBSTONE {
            return Err(DataError::Redacted(self.key.clone(), self.data.clone()));
        }

        let computed = compute_hgid(&self.data, self.parents);
        if computed != self.key.hgid {
            let err = InvalidHgId {
                expected: self.key.hgid,
                computed,
                data: self.data.clone(),
                parents: self.parents,
            };

            return Err(if self.key.path.is_empty() {
                DataError::Corrupt(err)
            } else {
                DataError::MaybeHybridManifest(err)
            });
        }

        Ok(self.data.clone())
    }

    /// Get this entry's data without verifying the hgid hash.
    pub fn data_unchecked(&self) -> Bytes {
        self.data.clone()
    }
}

fn compute_hgid(data: &[u8], parents: Parents) -> HgId {
    // Parents must be hashed in sorted order.
    let (p1, p2) = match parents.into_nodes() {
        (p1, p2) if p1 > p2 => (p2, p1),
        (p1, p2) => (p1, p2),
    };

    let mut hasher = Sha1::new();
    hasher.input(p1.as_ref());
    hasher.input(p2.as_ref());
    hasher.input(data);
    let hash: [u8; 20] = hasher.result().into();

    HgId::from_byte_array(hash)
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct DataRequest {
    pub keys: Vec<Key>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataResponse {
    pub entries: Vec<DataEntry>,
}

impl DataResponse {
    pub fn new(entries: impl IntoIterator<Item = DataEntry>) -> Self {
        Self::from_iter(entries)
    }
}

impl FromIterator<DataEntry> for DataResponse {
    fn from_iter<I: IntoIterator<Item = DataEntry>>(entries: I) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
}

impl IntoIterator for DataResponse {
    type Item = DataEntry;
    type IntoIter = std::vec::IntoIter<DataEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for DataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}
