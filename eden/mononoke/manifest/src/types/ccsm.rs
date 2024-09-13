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
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::case_conflict_skeleton_manifest::CaseConflictSkeletonManifest;
use mononoke_types::case_conflict_skeleton_manifest::CcsmEntry;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::MPathElement;

use super::Entry;
use super::Manifest;
use super::OrderedManifest;
use super::Weight;

pub(crate) fn ccsm_to_mf_entry(entry: CcsmEntry) -> Entry<CaseConflictSkeletonManifest, ()> {
    match entry {
        CcsmEntry::Directory(dir) => Entry::Tree(dir),
        CcsmEntry::File => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for CaseConflictSkeletonManifest {
    type TreeId = CaseConflictSkeletonManifest;
    type Leaf = ();
    type TrieMapType = LoadableShardedMapV2Node<CcsmEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, ccsm_to_mf_entry(entry)))
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
                .map_ok(|(path, entry)| (path, ccsm_to_mf_entry(entry)))
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
                .map_ok(|(path, entry)| (path, ccsm_to_mf_entry(entry)))
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
                .map_ok(|(path, entry)| (path, ccsm_to_mf_entry(entry)))
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
            .map(ccsm_to_mf_entry))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

fn convert_ccsm_to_weighted(
    entry: Entry<CaseConflictSkeletonManifest, ()>,
) -> Entry<(Weight, CaseConflictSkeletonManifest), ()> {
    match entry {
        Entry::Tree(dir) => Entry::Tree((
            dir.rollup_count()
                .into_inner()
                .try_into()
                .unwrap_or(usize::MAX),
            dir,
        )),
        Entry::Leaf(()) => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> OrderedManifest<Store> for CaseConflictSkeletonManifest {
    async fn list_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<'async_trait, Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::Leaf>)>>,
    > {
        self.list(ctx, blobstore).await.map(|stream| {
            stream
                .map_ok(|(p, entry)| (p, convert_ccsm_to_weighted(entry)))
                .boxed()
        })
    }

    async fn lookup_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::Leaf>>> {
        Manifest::lookup(self, ctx, blobstore, name)
            .await
            .map(|opt| opt.map(convert_ccsm_to_weighted))
    }
}
