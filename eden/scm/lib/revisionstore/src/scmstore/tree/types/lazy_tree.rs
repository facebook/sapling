/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use anyhow::Result;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use manifest_tree::TreeEntry as ManifestTreeEntry;
use minibytes::Bytes;
use storemodel::SerializationFormat;
use storemodel::TreeEntry as StoreModelTreeEntry;
use types::HgId;
use types::Id20;
use types::Parents;
use types::PathComponentBuf;
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
    IndexedLog(TreeEntryWithAux, SerializationFormat),

    /// An SaplingRemoteApi TreeEntry.
    SaplingRemoteApi(TreeEntry, bool, SerializationFormat),

    // Null tree is a special case with null content.
    Null,
}

pub enum AuxData {
    File(FileAuxData),
    Tree(TreeAuxData),
}

impl LazyTree {
    pub(crate) fn format(&self) -> SerializationFormat {
        match self {
            LazyTree::IndexedLog(_, format) | LazyTree::SaplingRemoteApi(_, _, format) => *format,
            LazyTree::Null => SerializationFormat::Hg,
        }
    }

    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyTree::*;
        match self {
            IndexedLog(entry_with_aux, ..) => Some(entry_with_aux.node()),
            SaplingRemoteApi(entry, ..) => Some(entry.key().hgid),
            Null => Some(NULL_ID),
        }
    }

    /// The tree content, as would be encoded in the Mercurial blob
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(entry_with_aux, ..) => entry_with_aux.content()?,
            SaplingRemoteApi(entry, verify_hash, ..) => entry.data(*verify_hash)?,
            Null => Bytes::default(),
        })
    }

    /// Convert the LazyTree to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, node: Id20) -> Result<Option<Entry>> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(entry_with_aux, ..) => Some(entry_with_aux.entry.clone()),
            SaplingRemoteApi(entry, verify_hash, format) => {
                let data = entry.data(*verify_hash)?;
                let mut cache_entry = Entry::new(node, data.clone(), Metadata::default());

                let acl_children = self.children_with_acl()?;
                if !acl_children.is_empty() {
                    let acl_hgids: HashSet<HgId> =
                        acl_children.iter().map(|(_, hgid)| *hgid).collect();
                    let manifest_entry = ManifestTreeEntry(data, *format);
                    let mut indices = Vec::new();
                    for (idx, elem) in manifest_entry.iter_owned()?.enumerate() {
                        let (_, hgid, _) = elem?;
                        if acl_hgids.contains(&hgid) {
                            indices.push(idx as u32);
                        }
                    }
                    if !indices.is_empty() {
                        cache_entry.set_acl_children_indices(indices);
                    }
                }

                Some(cache_entry)
            }
            Null => None,
        })
    }

    pub fn manifest_tree_entry(&self) -> Result<ManifestTreeEntry> {
        Ok(ManifestTreeEntry(self.hg_content()?, self.format()))
    }

    pub(crate) fn parents(&self) -> Option<Parents> {
        match &self {
            Self::SaplingRemoteApi(entry, ..) => entry.parents,
            _ => None,
        }
    }

    pub(crate) fn aux_data(&self) -> Result<Option<TreeAuxData>> {
        match &self {
            Self::IndexedLog(entry_with_aux, ..) => entry_with_aux.aux_data(),
            Self::SaplingRemoteApi(entry, ..) => Ok(entry.tree_aux_data.clone()),
            _ => Ok(None),
        }
    }

    pub fn children_aux_data(&self) -> Vec<(HgId, AuxData)> {
        use LazyTree::*;
        match self {
            SaplingRemoteApi(entry, ..) => {
                entry.children.as_ref().map_or_else(Vec::new, |children| {
                    children
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
                })
            }
            _ => Vec::new(),
        }
    }

    /// Returns `(path_component, manifest_id)` for directory children that have `has_acl` set.
    pub fn children_with_acl(&self) -> Result<Vec<(PathComponentBuf, HgId)>> {
        use LazyTree::*;
        match self {
            IndexedLog(entry_with_aux, ..) => {
                let indices = match entry_with_aux.entry.acl_children_indices() {
                    Some(indices) if !indices.is_empty() => indices,
                    _ => return Ok(Vec::new()),
                };
                let manifest_entry = self.manifest_tree_entry()?;
                let index_set: HashSet<u32> = indices.iter().copied().collect();
                let mut result = Vec::with_capacity(indices.len());
                for (idx, elem) in manifest_entry.iter_owned()?.enumerate() {
                    let (path, hgid, _) = elem?;
                    if index_set.contains(&(idx as u32)) {
                        result.push((path, hgid));
                    }
                }
                Ok(result)
            }
            SaplingRemoteApi(entry, ..) => {
                let children = match entry.children.as_ref() {
                    Some(children) => children,
                    None => return Ok(Vec::new()),
                };
                let mut result = Vec::new();
                for child in children {
                    let child_entry = child.as_ref().map_err(|e| e.clone())?;
                    if let TreeChildEntry::Directory(dir_entry) = child_entry {
                        if dir_entry.has_acl.unwrap_or(false) {
                            if let Some(path) = dir_entry.key.path.last_component() {
                                result.push((path.to_owned(), dir_entry.key.hgid));
                            }
                        }
                    }
                }
                Ok(result)
            }
            _ => Ok(Vec::new()),
        }
    }
}
