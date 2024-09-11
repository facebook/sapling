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
use futures::stream::TryStreamExt;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::test_manifest::TestManifest;
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::test_manifest::TestManifestEntry;
use mononoke_types::test_sharded_manifest::TestShardedManifest;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;

use super::Entry;
use super::Manifest;

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for TestManifest {
    type TreeId = TestManifestDirectory;
    type Leaf = ();
    type TrieMapType = SortedVectorTrieMap<Entry<TestManifestDirectory, ()>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self.lookup(name).map(convert_test_manifest))
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let values = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_test_manifest(entry)))
            .collect::<Vec<_>>();
        Ok(stream::iter(values).map(Ok).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .subentries
            .iter()
            .map(|(k, v)| (k.clone().to_smallvec(), convert_test_manifest(v)))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

fn convert_test_manifest(
    test_manifest_entry: &TestManifestEntry,
) -> Entry<TestManifestDirectory, ()> {
    match test_manifest_entry {
        TestManifestEntry::File => Entry::Leaf(()),
        TestManifestEntry::Directory(dir) => Entry::Tree(dir.clone()),
    }
}

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for TestShardedManifest {
    type TreeId = TestShardedManifestDirectory;
    type Leaf = ();
    type TrieMapType = LoadableShardedMapV2Node<TestShardedManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .map(convert_test_sharded_manifest))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

pub(crate) fn convert_test_sharded_manifest(
    test_sharded_manifest_entry: TestShardedManifestEntry,
) -> Entry<TestShardedManifestDirectory, ()> {
    match test_sharded_manifest_entry {
        TestShardedManifestEntry::File(_file) => Entry::Leaf(()),
        TestShardedManifestEntry::Directory(dir) => Entry::Tree(dir),
    }
}
