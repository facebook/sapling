/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::Arc;

use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use caching_ext::get_or_fill;
use caching_ext::get_or_fill_chunked;
use caching_ext::CacheDisposition;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::McErrorKind;
use caching_ext::McResult;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use commit_graph::edges::ChangesetEdges;
use commit_graph::edges::ChangesetNode;
use commit_graph::edges::ChangesetNodeParents;
use commit_graph::storage::CommitGraphStorage;
use commit_graph::storage::Prefetch;
use commit_graph_thrift as thrift;
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use maplit::hashset;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use ref_cast::RefCast;
use vec1::Vec1;

#[cfg(test)]
mod tests;

/// Size of chunk when fetching from the backing store
const CHUNK_SIZE: usize = 1000;

/// Number of chunks to fetch in parallel
const PARALLEL_CHUNKS: usize = 2;

/// Caching Commit Graph Storage
pub struct CachingCommitGraphStorage {
    storage: Arc<dyn CommitGraphStorage>,
    cachelib: CachelibHandler<CachedChangesetEdges>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
    repo_id: RepositoryId,
}

struct CacheRequest<'a> {
    ctx: &'a CoreContext,
    caching_storage: &'a CachingCommitGraphStorage,
    prefetch: Prefetch,
    required: bool,
}

#[derive(Clone, Debug, Abomonation, RefCast)]
#[repr(transparent)]
pub struct CachedChangesetEdges(ChangesetEdges);

impl Deref for CachedChangesetEdges {
    type Target = ChangesetEdges;

    fn deref(&self) -> &ChangesetEdges {
        &self.0
    }
}

impl CachedChangesetEdges {
    fn take(self) -> ChangesetEdges {
        self.0
    }

    fn node_to_thrift(node: &ChangesetNode) -> thrift::ChangesetNode {
        thrift::ChangesetNode {
            cs_id: node.cs_id.into_thrift(),
            generation: thrift::Generation(node.generation.value() as i64),
            skip_tree_depth: node.skip_tree_depth as i64,
            p1_linear_depth: node.p1_linear_depth as i64,
        }
    }

    fn to_thrift(&self) -> thrift::ChangesetEdges {
        thrift::ChangesetEdges {
            node: Self::node_to_thrift(&self.node),
            parents: self.parents.iter().map(Self::node_to_thrift).collect(),
            merge_ancestor: self.merge_ancestor.as_ref().map(Self::node_to_thrift),
            skip_tree_parent: self.skip_tree_parent.as_ref().map(Self::node_to_thrift),
            skip_tree_skew_ancestor: self
                .skip_tree_skew_ancestor
                .as_ref()
                .map(Self::node_to_thrift),
            p1_linear_skew_ancestor: self
                .p1_linear_skew_ancestor
                .as_ref()
                .map(Self::node_to_thrift),
        }
    }

    fn node_from_thrift(node: thrift::ChangesetNode) -> Result<ChangesetNode> {
        Ok(ChangesetNode {
            cs_id: ChangesetId::from_thrift(node.cs_id)?,
            generation: Generation::new(node.generation.0 as u64),
            skip_tree_depth: node.skip_tree_depth as u64,
            p1_linear_depth: node.p1_linear_depth as u64,
        })
    }

    fn from_thrift(edges: thrift::ChangesetEdges) -> Result<Self> {
        Ok(Self(ChangesetEdges {
            node: Self::node_from_thrift(edges.node)?,
            parents: edges
                .parents
                .into_iter()
                .map(Self::node_from_thrift)
                .collect::<Result<ChangesetNodeParents>>()?,
            merge_ancestor: edges
                .merge_ancestor
                .map(Self::node_from_thrift)
                .transpose()?,
            skip_tree_parent: edges
                .skip_tree_parent
                .map(Self::node_from_thrift)
                .transpose()?,
            skip_tree_skew_ancestor: edges
                .skip_tree_skew_ancestor
                .map(Self::node_from_thrift)
                .transpose()?,
            p1_linear_skew_ancestor: edges
                .p1_linear_skew_ancestor
                .map(Self::node_from_thrift)
                .transpose()?,
        }))
    }
}

impl MemcacheEntity for CachedChangesetEdges {
    fn serialize(&self) -> Bytes {
        compact_protocol::serialize(&self.to_thrift())
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        compact_protocol::deserialize(bytes)
            .and_then(CachedChangesetEdges::from_thrift)
            .map_err(|_| McErrorKind::Deserialization)
    }
}

impl EntityStore<CachedChangesetEdges> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<CachedChangesetEdges> {
        &self.caching_storage.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        &self.caching_storage.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        &self.caching_storage.memcache
    }

    fn cache_determinator(&self, _: &CachedChangesetEdges) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("commit_graph");

    #[cfg(test)]
    fn spawn_memcache_writes(&self) -> bool {
        match self.caching_storage.memcache {
            MemcacheHandler::Real(_) => true,
            MemcacheHandler::Mock(..) => false,
        }
    }
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, CachedChangesetEdges> for CacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        self.caching_storage.cache_key(cs_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, CachedChangesetEdges>> {
        let cs_ids: Vec<ChangesetId> = keys.into_iter().collect();
        let entries = if self.required {
            self.caching_storage
                .storage
                .fetch_many_edges_required(self.ctx, &cs_ids, self.prefetch.include_hint())
                .await?
        } else {
            self.caching_storage
                .storage
                .fetch_many_edges(self.ctx, &cs_ids, self.prefetch.include_hint())
                .await?
        };
        Ok(entries
            .into_iter()
            .map(|(cs_id, edges)| (cs_id, CachedChangesetEdges(edges)))
            .collect())
    }
}

impl CachingCommitGraphStorage {
    fn keygen() -> KeyGen {
        let key_prefix = "scm.mononoke.commitgraph";

        KeyGen::new(
            key_prefix,
            thrift::MC_CODEVER as u32,
            thrift::MC_SITEVER as u32,
        )
    }

    fn cache_key(&self, cs_id: &ChangesetId) -> String {
        format!("{}.{}", self.repo_id.prefix(), cs_id)
    }

    pub fn new(
        fb: FacebookInit,
        storage: Arc<dyn CommitGraphStorage>,
        cache_pool: cachelib::VolatileLruCachePool,
    ) -> Self {
        Self {
            repo_id: storage.repo_id(),
            storage,
            cachelib: cache_pool.into(),
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: Self::keygen(),
        }
    }

    #[cfg(test)]
    pub fn mocked(storage: Arc<dyn CommitGraphStorage>) -> Self {
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        Self {
            repo_id: storage.repo_id(),
            storage,
            cachelib,
            memcache,
            keygen: Self::keygen(),
        }
    }

    fn request<'a>(&'a self, ctx: &'a CoreContext, prefetch: Prefetch) -> CacheRequest<'a> {
        CacheRequest {
            ctx,
            caching_storage: self,
            prefetch,
            required: false,
        }
    }

    fn request_required<'a>(
        &'a self,
        ctx: &'a CoreContext,
        prefetch: Prefetch,
    ) -> CacheRequest<'a> {
        CacheRequest {
            ctx,
            caching_storage: self,
            prefetch,
            required: true,
        }
    }
}

#[async_trait]
impl CommitGraphStorage for CachingCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        self.storage.add(ctx, edges).await
    }

    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize> {
        self.storage.add_many(ctx, many_edges).await
    }

    async fn fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        let mut found = get_or_fill(self.request(ctx, Prefetch::None), hashset![cs_id]).await?;
        Ok(found.remove(&cs_id).map(CachedChangesetEdges::take))
    }

    async fn fetch_edges_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetEdges> {
        let mut found =
            get_or_fill(self.request_required(ctx, Prefetch::None), hashset![cs_id]).await?;
        Ok(found
            .remove(&cs_id)
            .ok_or_else(|| anyhow!("Missing changeset from commit graph storage: {}", cs_id))?
            .take())
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let cs_ids: HashSet<ChangesetId> = cs_ids.iter().copied().collect();
        let mut found = get_or_fill_chunked(
            self.request(ctx, prefetch),
            cs_ids.clone(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?;
        if prefetch.is_hint() {
            // We may have prefetched additional edges.  Remove them from the
            // result
            found.retain(|cs_id, _| cs_ids.contains(cs_id));
        }
        Ok(found
            .into_iter()
            .map(|(cs_id, edges)| (cs_id, edges.take()))
            .collect())
    }

    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let cs_ids: HashSet<ChangesetId> = cs_ids.iter().copied().collect();
        let mut found = get_or_fill_chunked(
            self.request_required(ctx, prefetch),
            cs_ids.clone(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?;
        if prefetch.is_hint() {
            // We may have prefetched additional edges.  Remove them from the
            // result
            found.retain(|cs_id, _| cs_ids.contains(cs_id));
        }
        Ok(found
            .into_iter()
            .map(|(cs_id, edges)| (cs_id, edges.take()))
            .collect())
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        self.storage.find_by_prefix(ctx, cs_prefix, limit).await
    }
}
