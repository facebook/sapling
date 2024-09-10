/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use mononoke_types::unode::ManifestUnode;
use mononoke_types::unode::UnodeEntry;
use mononoke_types::FileUnodeId;
use mononoke_types::MPathElement;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SortedVectorTrieMap;

use super::AsyncManifest;
use super::Entry;

#[async_trait]
impl<Store: Blobstore> AsyncManifest<Store> for ManifestUnode {
    type TreeId = ManifestUnodeId;
    type LeafId = FileUnodeId;
    type TrieMapType = SortedVectorTrieMap<Entry<ManifestUnodeId, FileUnodeId>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>> {
        Ok(self.lookup(name).map(convert_unode))
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        let values = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_unode(entry)))
            .collect::<Vec<_>>();
        Ok(stream::iter(values).map(Ok).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .subentries()
            .iter()
            .map(|(k, v)| (k.clone().to_smallvec(), convert_unode(v)))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

fn convert_unode(unode_entry: &UnodeEntry) -> Entry<ManifestUnodeId, FileUnodeId> {
    match unode_entry {
        UnodeEntry::File(file_unode_id) => Entry::Leaf(file_unode_id.clone()),
        UnodeEntry::Directory(mf_unode_id) => Entry::Tree(mf_unode_id.clone()),
    }
}
