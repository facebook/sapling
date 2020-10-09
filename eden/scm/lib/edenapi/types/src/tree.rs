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

use crate::{FileMetadata, FileMetadataRequest, InvalidHgId};

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

    #[error("TreeEntry missing field '{0}'")]
    MissingField(&'static str),
}

impl TreeError {
    /// Get the data anyway, despite the error.
    pub fn data(&self) -> Option<Bytes> {
        use TreeError::*;
        match self {
            Corrupt(InvalidHgId { data, .. }) => Some(data),
            MaybeHybridManifest(InvalidHgId { data, .. }) => Some(data),
            _ => None,
        }
        .cloned()
    }
}

/// Structure representing source control tree entry on the wire.
/// Includes the information required to add the data to a mutable store,
/// along with the parents for hash validation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeEntry {
    pub key: Key,
    pub data: Option<Bytes>,
    pub parents: Option<Parents>,
    pub file_metadata: Option<FileMetadata>,
}

impl TreeEntry {
    pub fn new(key: Key, data: Bytes, parents: Parents, metadata: Metadata) -> Self {
        Self {
            key,
            data: Some(data),
            parents: Some(parents),
            file_metadata: metadata.flags.map(|f| FileMetadata {
                revisionstore_flags: Some(f),
            }),
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
            MaybeHybridManifest(_) => Ok(e
                .data()
                .expect("TreeError::MaybeHybridManifest should always carry underlying data")),
            _ => Err(e),
        })
    }

    /// Get this entry's data after verifying the hgid hash.
    pub fn data_checked(&self) -> Result<Bytes, TreeError> {
        if let Some(data) = self.data.as_ref() {
            if let Some(parents) = self.parents {
                let computed = HgId::from_content(&data, parents);
                if computed != self.key.hgid {
                    let err = InvalidHgId {
                        expected: self.key.hgid,
                        computed,
                        data: data.clone(),
                        parents,
                    };

                    return Err(if self.key.path.is_empty() {
                        TreeError::MaybeHybridManifest(err)
                    } else {
                        TreeError::Corrupt(err)
                    });
                }

                Ok(data.clone())
            } else {
                Err(TreeError::MissingField("parents"))
            }
        } else {
            Err(TreeError::MissingField("data"))
        }
    }

    /// Get this entry's data without verifying the hgid hash.
    pub fn data_unchecked(&self) -> Option<Bytes> {
        self.data.clone()
    }

    /// Get this entry's revisionstore metadata.
    pub fn metadata(&self) -> Metadata {
        Metadata {
            flags: self.file_metadata.and_then(|m| m.revisionstore_flags),
            size: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let bytes: Option<Vec<u8>> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: bytes.map(|b| Bytes::from(b)),
            parents: Arbitrary::arbitrary(g),
            file_metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeRequest {
    pub keys: Vec<Key>,
    pub with_file_metadata: Option<FileMetadataRequest>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            with_file_metadata: Arbitrary::arbitrary(g),
        }
    }
}
