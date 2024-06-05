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
use types::HgId;
use types::Key;

use crate::indexedlogdatastore::Entry;
use crate::scmstore::file::FileAuxData;
use crate::Metadata;

/// A minimal tree enum that simply wraps the possible underlying tree types,
/// with no processing.
#[derive(Debug)]
pub(crate) enum LazyTree {
    /// A response from calling into the legacy storage API
    ContentStore(Bytes, Metadata),

    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// An SaplingRemoteApi TreeEntry.
    SaplingRemoteApi(TreeEntry),
}

impl LazyTree {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyTree::*;
        match self {
            ContentStore(_, _) => None,
            IndexedLog(ref entry) => Some(entry.key().hgid),
            SaplingRemoteApi(ref entry) => Some(entry.key().hgid),
        }
    }

    /// The tree content, as would be encoded in the Mercurial blob
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.content()?,
            ContentStore(ref blob, _) => blob.clone(),
            SaplingRemoteApi(ref entry) => entry.data()?.into(),
        })
    }

    /// Convert the LazyTree to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            SaplingRemoteApi(ref entry) => {
                Some(Entry::new(key, entry.data()?.into(), Metadata::default()))
            }
            // ContentStore handles caching internally
            ContentStore(_, _) => None,
        })
    }

    pub fn manifest_tree_entry(&mut self) -> Result<ManifestTreeEntry> {
        // TODO(meyer): Make manifest-tree crate use minibytes::Bytes
        // Currently revisionstore is only for hg format.
        let format = SerializationFormat::Hg;
        Ok(ManifestTreeEntry(self.hg_content()?, format))
    }

    pub fn aux_data(&self) -> HashMap<HgId, FileAuxData> {
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
                                let child_entry = match entry {
                                    Err(err) => {
                                        tracing::warn!("Error fetching child entry: {:?}", err);
                                        return None;
                                    }
                                    Ok(file_entry) => file_entry,
                                };
                                // TODO: Also return directory aux data.
                                if let TreeChildEntry::File(file_entry) = child_entry {
                                    file_entry.file_metadata.map(|file_metadata| {
                                        (file_entry.key.hgid, file_metadata.into())
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect::<HashMap<_, _>>()
                    })
            }
            _ => HashMap::new(),
        }
    }
}
