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
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;

use super::Entry;
use super::Manifest;
use super::OrderedManifest;
use super::Weight;

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for Fsnode {
    type TreeId = FsnodeId;
    type Leaf = FsnodeFile;
    type TrieMapType = SortedVectorTrieMap<Entry<FsnodeId, FsnodeFile>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self.lookup(name).map(convert_fsnode))
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let values = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode(entry)))
            .collect::<Vec<_>>();
        Ok(stream::iter(values).map(Ok).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .into_subentries()
            .iter()
            .map(|(k, v)| (k.clone().to_smallvec(), convert_fsnode(v)))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

fn convert_fsnode(fsnode_entry: &FsnodeEntry) -> Entry<FsnodeId, FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => Entry::Tree(fsnode_directory.id().clone()),
    }
}

#[async_trait]
impl<Store: Blobstore> OrderedManifest<Store> for Fsnode {
    async fn lookup_weighted(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::Leaf>>> {
        Ok(self.lookup(name).map(convert_fsnode_weighted))
    }

    async fn list_weighted(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<
        BoxStream<'async_trait, Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::Leaf>)>>,
    > {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode_weighted(entry)))
            .collect();
        Ok(stream::iter(v).map(Ok).boxed())
    }
}

fn convert_fsnode_weighted(fsnode_entry: &FsnodeEntry) -> Entry<(Weight, FsnodeId), FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => {
            let summary = fsnode_directory.summary();
            // Fsnodes don't have a full descendant dirs count, so we use the
            // child count as a lower-bound estimate.
            let weight = summary.descendant_files_count + summary.child_dirs_count;
            Entry::Tree((weight as Weight, fsnode_directory.id().clone()))
        }
    }
}
