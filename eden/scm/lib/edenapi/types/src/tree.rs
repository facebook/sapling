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
use serde_derive::Deserialize;
use serde_derive::Serialize;
use thiserror::Error;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::key::Key;
use types::parents::Parents;

use crate::DirectoryMetadata;
use crate::EdenApiServerError;
use crate::FileMetadata;
use crate::InvalidHgId;
use crate::UploadToken;

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
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct TreeEntry {
    pub key: Key,
    pub data: Option<Bytes>,
    pub parents: Option<Parents>,
    #[serde(skip)]
    pub children: Option<Vec<Result<TreeChildEntry, EdenApiServerError>>>,
}

impl TreeEntry {
    pub fn new(key: Key, data: Bytes, parents: Parents) -> Self {
        Self {
            key,
            data: Some(data),
            parents: Some(parents),
            children: None,
        }
    }

    pub fn with_children<'a>(
        &'a mut self,
        children: Option<Vec<Result<TreeChildEntry, EdenApiServerError>>>,
    ) -> &'a mut Self {
        self.children = children;
        self
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeChildEntry {
    File(TreeChildFileEntry),
    Directory(TreeChildDirectoryEntry),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreeChildFileEntry {
    pub key: Key,
    pub file_metadata: Option<FileMetadata>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreeChildDirectoryEntry {
    pub key: Key,
    pub directory_metadata: Option<DirectoryMetadata>,
}

impl TreeChildEntry {
    pub fn new_file_entry(key: Key, metadata: FileMetadata) -> Self {
        TreeChildEntry::File(TreeChildFileEntry {
            key,
            file_metadata: Some(metadata),
        })
    }

    pub fn new_directory_entry(key: Key) -> Self {
        TreeChildEntry::Directory(TreeChildDirectoryEntry {
            key,
            ..Default::default()
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Option<Vec<u8>> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: bytes.map(|b| Bytes::from(b)),
            parents: Arbitrary::arbitrary(g),
            // Recursive TreeEntry in children causes stack overflow in QuickCheck
            children: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeChildEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        match bool::arbitrary(g) {
            true => TreeChildEntry::File(Arbitrary::arbitrary(g)),
            false => TreeChildEntry::Directory(Arbitrary::arbitrary(g)),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeChildFileEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            file_metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeChildDirectoryEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            directory_metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeRequest {
    pub keys: Vec<Key>,
    pub attributes: TreeAttributes,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct TreeAttributes {
    #[serde(default = "get_true")]
    pub manifest_blob: bool,
    #[serde(default = "get_true")]
    pub parents: bool,
    #[serde(default = "get_true")]
    pub child_metadata: bool,
}

fn get_true() -> bool {
    true
}

impl TreeAttributes {
    pub fn all() -> Self {
        TreeAttributes {
            manifest_blob: true,
            parents: true,
            child_metadata: true,
        }
    }
}

impl Default for TreeAttributes {
    fn default() -> Self {
        TreeAttributes {
            manifest_blob: true,
            parents: true,
            child_metadata: false,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeAttributes {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            manifest_blob: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            child_metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            attributes: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UploadTreeEntry {
    #[id(0)]
    pub node_id: HgId,
    #[id(1)]
    pub data: Vec<u8>,
    #[id(2)]
    pub parents: Parents,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadTreeRequest {
    #[id(0)]
    pub entry: UploadTreeEntry,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadTreeResponse {
    #[id(1)]
    pub token: UploadToken,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTreeResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            token: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTreeEntry {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            node_id: Arbitrary::arbitrary(g),
            data: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadTreeRequest {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            entry: Arbitrary::arbitrary(g),
        }
    }
}
