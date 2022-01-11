/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use edenapi_types::TreeEntry;
use manifest_tree::TreeEntry as ManifestTreeEntry;
use minibytes::Bytes;
use storemodel::TreeFormat;
use tracing::instrument;
use types::HgId;
use types::Key;

use crate::indexedlogdatastore::Entry;
use crate::memcache::McData;
use crate::Metadata;

/// A minimal tree enum that simply wraps the possible underlying tree types,
/// with no processing.
#[derive(Debug)]
pub(crate) enum LazyTree {
    /// A response from calling into the legacy storage API
    ContentStore(Bytes, Metadata),

    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// An EdenApi TreeEntry.
    EdenApi(TreeEntry),

    /// A memcache entry, convertable to Entry. In this case the Key's path should match the requested Key's path.
    Memcache(McData),
}

impl LazyTree {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyTree::*;
        match self {
            ContentStore(_, _) => None,
            IndexedLog(ref entry) => Some(entry.key().hgid),
            EdenApi(ref entry) => Some(entry.key().hgid),
            Memcache(ref entry) => Some(entry.key.hgid),
        }
    }

    /// The tree content, as would be encoded in the Mercurial blob
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn hg_content(&mut self) -> Result<Bytes> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(ref mut entry) => entry.content()?,
            ContentStore(ref blob, _) => blob.clone(),
            EdenApi(ref entry) => entry.data()?.into(),
            Memcache(ref entry) => entry.data.clone(),
        })
    }

    /// Convert the LazyTree to an indexedlog Entry, if it should ever be written to IndexedLog cache
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyTree::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            EdenApi(ref entry) => Some(Entry::new(key, entry.data()?.into(), Metadata::default())),
            // TODO(meyer): We shouldn't ever need to replace the key with Memcache, can probably just clone this.
            Memcache(ref entry) => Some({
                let entry: Entry = entry.clone().into();
                entry.with_key(key)
            }),
            // ContentStore handles caching internally
            ContentStore(_, _) => None,
        })
    }

    pub fn manifest_tree_entry(&mut self) -> Result<ManifestTreeEntry> {
        // TODO(meyer): Make manifest-tree crate use minibytes::Bytes
        // Currently revisionstore is only for hg format.
        let format = TreeFormat::Hg;
        Ok(ManifestTreeEntry(
            self.hg_content()?.into_vec().into(),
            format,
        ))
    }
}
