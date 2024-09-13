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
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Directory;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Entry;
use mononoke_types::case_conflict_skeleton_manifest::CaseConflictSkeletonManifest;
use mononoke_types::case_conflict_skeleton_manifest::CcsmEntry;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2Entry;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::SortedVectorTrieMap;
use mononoke_types::TrieMap;
use smallvec::SmallVec;

use crate::types::Entry;

#[async_trait]
pub trait TrieMapOps<Store, Value>: Sized {
    async fn expand(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<(Option<Value>, Vec<(u8, Self)>)>;

    async fn into_stream(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(SmallVec<[u8; 24]>, Value)>>>;

    fn is_empty(&self) -> bool;
}

#[async_trait]
impl<Store, V: Send> TrieMapOps<Store, V> for TrieMap<V> {
    async fn expand(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<(Option<V>, Vec<(u8, Self)>)> {
        Ok(self.expand())
    }

    async fn into_stream(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(SmallVec<[u8; 24]>, V)>>> {
        Ok(stream::iter(self).map(Ok).boxed())
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

#[async_trait]
impl<Store, V: Clone + Send + Sync> TrieMapOps<Store, V> for SortedVectorTrieMap<V> {
    async fn expand(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<(Option<V>, Vec<(u8, Self)>)> {
        SortedVectorTrieMap::expand(self)
    }

    async fn into_stream(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(SmallVec<[u8; 24]>, V)>>> {
        Ok(stream::iter(self).map(Ok).boxed())
    }

    fn is_empty(&self) -> bool {
        SortedVectorTrieMap::is_empty(self)
    }
}

#[async_trait]
impl<Store: Blobstore> TrieMapOps<Store, Entry<TestShardedManifestDirectory, ()>>
    for LoadableShardedMapV2Node<TestShardedManifestEntry>
{
    async fn expand(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<(
        Option<Entry<TestShardedManifestDirectory, ()>>,
        Vec<(u8, Self)>,
    )> {
        let (entry, children) = self.expand(ctx, blobstore).await?;
        Ok((
            entry.map(crate::types::convert_test_sharded_manifest),
            children,
        ))
    }

    async fn into_stream(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(SmallVec<[u8; 24]>, Entry<TestShardedManifestDirectory, ()>)>,
        >,
    > {
        Ok(self
            .load(ctx, blobstore)
            .await?
            .into_entries(ctx, blobstore)
            .map_ok(|(k, v)| (k, crate::types::convert_test_sharded_manifest(v)))
            .boxed())
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }
}

#[async_trait]
impl<Store: Blobstore> TrieMapOps<Store, Entry<BssmV3Directory, ()>>
    for LoadableShardedMapV2Node<BssmV3Entry>
{
    async fn expand(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<(Option<Entry<BssmV3Directory, ()>>, Vec<(u8, Self)>)> {
        let (entry, children) = self.expand(ctx, blobstore).await?;
        Ok((entry.map(crate::types::bssm_v3_to_mf_entry), children))
    }

    async fn into_stream(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(SmallVec<[u8; 24]>, Entry<BssmV3Directory, ()>)>>>
    {
        Ok(self
            .load(ctx, blobstore)
            .await?
            .into_entries(ctx, blobstore)
            .map_ok(|(k, v)| (k, crate::types::bssm_v3_to_mf_entry(v)))
            .boxed())
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }
}

#[async_trait]
impl<Store: Blobstore> TrieMapOps<Store, Entry<SkeletonManifestV2, ()>>
    for LoadableShardedMapV2Node<SkeletonManifestV2Entry>
{
    async fn expand(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<(Option<Entry<SkeletonManifestV2, ()>>, Vec<(u8, Self)>)> {
        let (entry, children) = self.expand(ctx, blobstore).await?;
        Ok((
            entry.map(crate::types::skeleton_manifest_v2_to_mf_entry),
            children,
        ))
    }

    async fn into_stream(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(SmallVec<[u8; 24]>, Entry<SkeletonManifestV2, ()>)>>>
    {
        Ok(self
            .load(ctx, blobstore)
            .await?
            .into_entries(ctx, blobstore)
            .map_ok(|(k, v)| (k, crate::types::skeleton_manifest_v2_to_mf_entry(v)))
            .boxed())
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }
}

#[async_trait]
impl<Store: Blobstore> TrieMapOps<Store, Entry<CaseConflictSkeletonManifest, ()>>
    for LoadableShardedMapV2Node<CcsmEntry>
{
    async fn expand(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<(
        Option<Entry<CaseConflictSkeletonManifest, ()>>,
        Vec<(u8, Self)>,
    )> {
        let (entry, children) = self.expand(ctx, blobstore).await?;
        Ok((entry.map(crate::types::ccsm_to_mf_entry), children))
    }

    async fn into_stream(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(SmallVec<[u8; 24]>, Entry<CaseConflictSkeletonManifest, ()>)>,
        >,
    > {
        Ok(self
            .load(ctx, blobstore)
            .await?
            .into_entries(ctx, blobstore)
            .map_ok(|(k, v)| (k, crate::types::ccsm_to_mf_entry(v)))
            .boxed())
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }
}
