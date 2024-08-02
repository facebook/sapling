/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use minibytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use thiserror::Error;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::hgid::NULL_ID;
use types::key::Key;
use types::parents::Parents;
use types::AugmentedTree;
use types::AugmentedTreeEntry;
use types::AugmentedTreeWithDigest;

use crate::Blake3;
use crate::DirectoryMetadata;
use crate::FileAuxData;
use crate::FileMetadata;
use crate::InvalidHgId;
use crate::SaplingRemoteApiServerError;
use crate::Sha1;
use crate::UploadToken;

pub type TreeAuxData = DirectoryMetadata;

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

    #[error("TreeEntry failed to convert from AugmentedTree: '{0}'")]
    AugmentedTreeConversionError(String),
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
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TreeEntry {
    pub key: Key,
    pub data: Option<Bytes>,
    pub parents: Option<Parents>,
    pub children: Option<Vec<Result<TreeChildEntry, SaplingRemoteApiServerError>>>,
    pub tree_aux_data: Option<TreeAuxData>,
}

impl TreeEntry {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            ..Default::default()
        }
    }

    pub fn with_data<'a>(&'a mut self, data: Option<Bytes>) -> &'a mut Self {
        self.data = data;
        self
    }

    pub fn with_parents<'a>(&'a mut self, parents: Option<Parents>) -> &'a mut Self {
        self.parents = parents;
        self
    }

    pub fn with_children<'a>(
        &'a mut self,
        children: Option<Vec<Result<TreeChildEntry, SaplingRemoteApiServerError>>>,
    ) -> &'a mut Self {
        self.children = children;
        self
    }

    pub fn with_tree_aux_data<'a>(&'a mut self, tree_aux_data: TreeAuxData) -> &'a mut Self {
        self.tree_aux_data = Some(tree_aux_data);
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
                let computed = HgId::from_content(data, parents);
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

    pub fn tree_aux_data(&self) -> Option<&TreeAuxData> {
        self.tree_aux_data.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum TreeChildEntry {
    File(TreeChildFileEntry),
    Directory(TreeChildDirectoryEntry),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct TreeChildFileEntry {
    pub key: Key,
    pub file_metadata: Option<FileMetadata>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct TreeChildDirectoryEntry {
    pub key: Key,
    pub tree_aux_data: Option<TreeAuxData>,
}

impl TreeChildEntry {
    pub fn new_file_entry(key: Key, metadata: FileMetadata) -> Self {
        TreeChildEntry::File(TreeChildFileEntry {
            key,
            file_metadata: Some(metadata),
        })
    }

    pub fn new_directory_entry(key: Key, aux_data: TreeAuxData) -> Self {
        TreeChildEntry::Directory(TreeChildDirectoryEntry {
            key,
            tree_aux_data: Some(aux_data),
        })
    }
}

impl TryFrom<AugmentedTree> for TreeEntry {
    type Error = TreeError;
    fn try_from(aug_tree: AugmentedTree) -> Result<Self, Self::Error> {
        let mut entry: TreeEntry = TreeEntry::new(Key {
            hgid: aug_tree.hg_node_id,
            ..Default::default()
        });
        let mut buf: Vec<u8> = Vec::with_capacity(aug_tree.sapling_tree_blob_size());
        aug_tree
            .write_sapling_tree_blob(&mut buf)
            .map_err(|e| TreeError::AugmentedTreeConversionError(e.to_string()))?;
        entry.with_data(Some(buf.into()));
        entry.with_parents(Some(Parents::new(
            aug_tree.p1.unwrap_or(NULL_ID),
            aug_tree.p2.unwrap_or(NULL_ID),
        )));
        entry.with_children(Some(
            aug_tree
                .entries
                .into_iter()
                .map(|(path, augmented_entry)| match augmented_entry {
                    AugmentedTreeEntry::FileNode(file) => Ok(TreeChildEntry::new_file_entry(
                        Key {
                            hgid: file.filenode,
                            path,
                        },
                        FileAuxData {
                            blake3: Blake3::from_another(file.content_blake3),
                            sha1: Sha1::from_another(file.content_sha1),
                            total_size: file.total_size,
                            // in FileAuxData None would mean file_header_metadata is not fetched/not known if it is present
                            file_header_metadata: Some(
                                file.file_header_metadata.unwrap_or_default(),
                            ),
                        }
                        .into(),
                    )),
                    AugmentedTreeEntry::DirectoryNode(tree) => {
                        Ok(TreeChildEntry::new_directory_entry(
                            Key {
                                hgid: tree.treenode,
                                path,
                            },
                            DirectoryMetadata {
                                augmented_manifest_id: Blake3::from_another(
                                    tree.augmented_manifest_id,
                                ),
                                augmented_manifest_size: tree.augmented_manifest_size,
                            },
                        ))
                    }
                })
                .collect::<Result<Vec<_>, TreeError>>()?
                .into_iter()
                .map(Ok)
                .collect(),
        ));
        Ok(entry)
    }
}

impl TryFrom<AugmentedTreeWithDigest> for TreeEntry {
    type Error = TreeError;
    fn try_from(aug_tree_with_digest: AugmentedTreeWithDigest) -> Result<Self, Self::Error> {
        let mut entry: TreeEntry = TreeEntry::try_from(aug_tree_with_digest.augmented_tree)?;
        let dir_meta = DirectoryMetadata {
            augmented_manifest_id: Blake3::from_byte_array(
                aug_tree_with_digest.augmented_manifest_id.into_byte_array(),
            ),
            augmented_manifest_size: aug_tree_with_digest.augmented_manifest_size,
        };
        entry.with_tree_aux_data(dir_meta);
        Ok(entry)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for TreeEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Option<Vec<u8>> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: bytes.map(Bytes::from),
            parents: Arbitrary::arbitrary(g),
            // Recursive TreeEntry in children causes stack overflow in QuickCheck
            children: None,
            tree_aux_data: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct TreeRequest {
    pub keys: Vec<Key>,
    pub attributes: TreeAttributes,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct TreeAttributes {
    #[serde(default = "get_true")]
    pub manifest_blob: bool,
    #[serde(default = "get_true")]
    pub parents: bool,
    #[serde(default = "get_true")]
    pub child_metadata: bool,
    #[serde(default = "get_false")]
    pub augmented_trees: bool,
}

fn get_true() -> bool {
    true
}

fn get_false() -> bool {
    false
}

impl TreeAttributes {
    pub fn all() -> Self {
        TreeAttributes {
            manifest_blob: true,
            parents: true,
            child_metadata: true,
            augmented_trees: false,
        }
    }

    pub fn augmented_trees() -> Self {
        TreeAttributes {
            manifest_blob: false,  // not used
            parents: false,        // not used
            child_metadata: false, // not used
            augmented_trees: true,
        }
    }
}

impl Default for TreeAttributes {
    fn default() -> Self {
        TreeAttributes {
            manifest_blob: true,
            parents: true,
            child_metadata: false,
            augmented_trees: false,
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
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
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadTreeRequest {
    #[id(0)]
    pub entry: UploadTreeEntry,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadTreeResponse {
    #[id(1)]
    pub token: UploadToken,
}
