/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use manifest_augmented_tree::AugmentedTreeEntry;
use manifest_augmented_tree::AugmentedTreeWithDigest;
use manifest_tree::TreeEntry as ManifestTreeEntry;
use minibytes::Bytes;
use storemodel::SerializationFormat;
use types::HgId;
use types::Id20;
use types::Parents;
use types::hgid::NULL_ID;

use crate::Metadata;
use crate::indexedlogdatastore::Entry;
use crate::scmstore::file::FileAuxData;
use crate::scmstore::tree::TreeAuxData;
use crate::scmstore::tree::TreeEntryWithAux;

/// A minimal tree enum that simply wraps the possible underlying tree types,
/// with no processing.
#[derive(Debug, Clone)]
pub(crate) enum LazyTree {
    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    /// It may include the tree aux data if available
    IndexedLog(TreeEntryWithAux),

    /// An SaplingRemoteApi TreeEntry.
    SaplingRemoteApi(TreeEntry),

    // Null tree is a special case with null content.
    Null,
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
            IndexedLog(entry_with_aux) => Some(entry_with_aux.node()),
            SaplingRemoteApi(entry) => Some(entry.key().hgid),
            Null => Some(NULL_ID),
        }
    }

    /// The tree content, as would be encoded in the Mercurial blob
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(entry_with_aux) => entry_with_aux.content()?,
            SaplingRemoteApi(entry) => entry.data()?,
            Null => Bytes::default(),
        })
    }

    /// Convert the LazyTree to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, node: Id20) -> Result<Option<Entry>> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(entry_with_aux) => Some(entry_with_aux.entry.clone()),
            SaplingRemoteApi(entry) => Some(Entry::new(node, entry.data()?, Metadata::default())),
            Null => None,
        })
    }

    pub fn manifest_tree_entry(&self) -> Result<ManifestTreeEntry> {
        // Currently revisionstore is only for hg format.
        Ok(ManifestTreeEntry(
            self.hg_content()?,
            SerializationFormat::Hg,
        ))
    }

    pub(crate) fn parents(&self) -> Option<Parents> {
        match &self {
            Self::SaplingRemoteApi(entry) => entry.parents,
            _ => None,
        }
    }

    pub(crate) fn aux_data(&self) -> Option<TreeAuxData> {
        match &self {
            Self::IndexedLog(entry_with_aux) => entry_with_aux.tree_aux.clone(),
            Self::SaplingRemoteApi(entry) => entry.tree_aux_data.clone(),
            _ => None,
        }
    }

    pub fn children_aux_data(&self) -> Vec<(HgId, AuxData)> {
        use LazyTree::*;
        match self {
            SaplingRemoteApi(entry) => entry.children.as_ref().map_or_else(Vec::new, |childrens| {
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
                                    (file_entry.key.hgid, AuxData::File(file_metadata.into()))
                                })
                            }
                            TreeChildEntry::Directory(dir_entry) => {
                                dir_entry.tree_aux_data.map(|dir_metadata| {
                                    (dir_entry.key.hgid, AuxData::Tree(dir_metadata))
                                })
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }),
            _ => Vec::new(),
        }
    }
}
