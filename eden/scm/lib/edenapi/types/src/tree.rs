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

#[derive(Debug, Error)]
pub enum TreeError {
    #[error(transparent)]
    Corrupt(InvalidHgId),
    /// If this entry represents a root node (i.e., has an empty path), it
    /// may be a hybrid tree manifest which has the content of a root tree
    /// manifest node, but the hash of the corresponding flat manifest. This
    /// situation should only occur for manifests created in "hybrid mode"
    /// (i.e., during a transition from flat manifests to tree manifests).
    #[error("Detected possible hybrid manifest: {0}")]
    MaybeHybridManifest(#[source] InvalidHgId),
}

impl TreeError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Bytes {
        use TreeError::*;
        match self {
            Corrupt(InvalidHgId { data, .. }) => data,
            MaybeHybridManifest(InvalidHgId { data, .. }) => data,
        }
        .clone()
    }
}

/// Structure representing source control tree entry on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeEntry {
    pub key: Key,
    pub data: Bytes,
    pub parents: Parents,
    pub metadata: Metadata,
}

impl TreeEntry {
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
    /// if this is a suspected hybrid manifest.
    pub fn data(&self) -> Result<Bytes, TreeError> {
        use TreeError::*;
        self.data_checked().or_else(|e| match e {
            Corrupt(_) => Err(e),
            MaybeHybridManifest(_) => Ok(e.data()),
        })
    }

    /// Get this entry's data after verifying the hgid hash.
    pub fn data_checked(&self) -> Result<Bytes, TreeError> {
        let computed = HgId::from_content(&self.data, self.parents);
        if computed != self.key.hgid {
            let err = InvalidHgId {
                expected: self.key.hgid,
                computed,
                data: self.data.clone(),
                parents: self.parents,
            };

            return Err(if self.key.path.is_empty() {
                TreeError::MaybeHybridManifest(err)
            } else {
                TreeError::Corrupt(err)
            });
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeEntry {
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeRequest {
    pub keys: Vec<Key>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}
