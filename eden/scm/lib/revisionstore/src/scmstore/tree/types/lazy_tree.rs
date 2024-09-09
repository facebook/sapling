/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use manifest_tree::TreeEntry as ManifestTreeEntry;
use minibytes::Bytes;
use storemodel::SerializationFormat;
use types::hgid::NULL_ID;
use types::AugmentedTreeEntry;
use types::AugmentedTreeWithDigest;
use types::HgId;
use types::Key;
use types::Parents;

use crate::indexedlogdatastore::Entry;
use crate::scmstore::file::FileAuxData;
use crate::scmstore::tree::TreeAuxData;
use crate::Metadata;

/// A minimal tree enum that simply wraps the possible underlying tree types,
/// with no processing.
#[derive(Debug)]
pub(crate) enum LazyTree {
    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// An SaplingRemoteApi TreeEntry.
    SaplingRemoteApi(TreeEntry),

    /// Tree data from CAS. Note that CAS actually contains AugmentedTree (without
    /// digest), but we have the digest in-hand so we store an AugmentedTreeWithDigest.
    Cas(AugmentedTreeWithDigest),
}

pub enum AuxData {
    File(FileAuxData),
    Tree(TreeAuxData),
}

impl LazyTree {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyTree::*;
        match self {
            IndexedLog(entry) => Some(entry.key().hgid),
            SaplingRemoteApi(entry) => Some(entry.key().hgid),
            Cas(entry) => Some(entry.augmented_tree.hg_node_id),
        }
    }

    /// The tree content, as would be encoded in the Mercurial blob
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(entry) => entry.content()?,
            SaplingRemoteApi(entry) => entry.data()?,
            Cas(entry) => {
                let tree = &entry.augmented_tree;
                let mut data = Vec::with_capacity(tree.sapling_tree_blob_size());
                tree.write_sapling_tree_blob(&mut data)?;
                data.into()
            }
        })
    }

    /// Convert the LazyTree to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            SaplingRemoteApi(ref entry) => {
                Some(Entry::new(key, entry.data()?, Metadata::default()))
            }
            // Don't write CAS entries to local cache.
            Cas(_) => None,
        })
    }

    pub fn manifest_tree_entry(&mut self) -> Result<ManifestTreeEntry> {
        // TODO(meyer): Make manifest-tree crate use minibytes::Bytes
        // Currently revisionstore is only for hg format.
        let format = SerializationFormat::Hg;
        Ok(ManifestTreeEntry(self.hg_content()?, format))
    }

    pub(crate) fn parents(&self) -> Option<Parents> {
        match &self {
            Self::SaplingRemoteApi(entry) => entry.parents,
            Self::Cas(entry) => Some(Parents::new(
                entry.augmented_tree.p1.unwrap_or(NULL_ID),
                entry.augmented_tree.p2.unwrap_or(NULL_ID),
            )),
            _ => None,
        }
    }

    pub(crate) fn aux_data(&self) -> Option<TreeAuxData> {
        match &self {
            Self::SaplingRemoteApi(entry) => entry.tree_aux_data.clone(),
            Self::Cas(entry) => Some(TreeAuxData {
                augmented_manifest_id: entry.augmented_manifest_id,
                augmented_manifest_size: entry.augmented_manifest_size,
            }),
            _ => None,
        }
    }

    pub fn children_aux_data(&self) -> HashMap<HgId, AuxData> {
        use LazyTree::*;
        match self {
            SaplingRemoteApi(entry) => {
                entry
                    .children
                    .as_ref()
                    .map_or_else(HashMap::new, |childrens| {
                        childrens
                            .iter()
                            .filter_map(|entry| {
                                let child_entry = entry
                                    .as_ref()
                                    .inspect_err(|err| {
                                        tracing::warn!("Error fetching child entry: {:?}", err);
                                    })
                                    .ok()?;
                                match child_entry {
                                    TreeChildEntry::File(file_entry) => {
                                        file_entry.file_metadata.clone().map(|file_metadata| {
                                            (
                                                file_entry.key.hgid,
                                                AuxData::File(file_metadata.into()),
                                            )
                                        })
                                    }
                                    TreeChildEntry::Directory(dir_entry) => {
                                        dir_entry.tree_aux_data.map(|dir_metadata| {
                                            (dir_entry.key.hgid, AuxData::Tree(dir_metadata))
                                        })
                                    }
                                }
                            })
                            .collect::<HashMap<_, _>>()
                    })
            }
            Cas(entry) => entry
                .augmented_tree
                .entries
                .iter()
                .map(|(_path, child)| match child {
                    AugmentedTreeEntry::FileNode(file) => (
                        file.filenode,
                        AuxData::File(FileAuxData {
                            total_size: file.total_size,
                            sha1: file.content_sha1,
                            blake3: file.content_blake3,
                            file_header_metadata: Some(
                                file.file_header_metadata.clone().unwrap_or_default(),
                            ),
                        }),
                    ),
                    AugmentedTreeEntry::DirectoryNode(dir) => (
                        dir.treenode,
                        AuxData::Tree(TreeAuxData {
                            augmented_manifest_id: dir.augmented_manifest_id,
                            augmented_manifest_size: dir.augmented_manifest_size,
                        }),
                    ),
                })
                .collect(),
            _ => HashMap::new(),
        }
    }
}
