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
use caching_ext::fill_cache;
use caching_ext::get_or_fill;
use caching_ext::get_or_fill_chunked;
use caching_ext::CacheDisposition;
use caching_ext::CacheHandlerFactory;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::McErrorKind;
use caching_ext::McResult;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use commit_graph_thrift as thrift;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use fbthrift::compact_protocol;
use maplit::hashset;
use memcache::KeyGen;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use stats::prelude::*;
use vec1::Vec1;

#[cfg(test)]
mod tests;

define_stats! {
    prefix = "mononoke.cache.commit_graph.prefetch";

    hit: timeseries("hit"; Rate, Sum),
    fetched: timeseries("fetched"; Rate, Sum),
    prefetched: timeseries("prefetched"; Rate, Sum),
}

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

#[derive(Clone, Debug, Abomonation)]
pub struct CachedChangesetEdges {
    /// The cached edges.
    edges: ChangesetEdges,

    /// Whether these edges were originally fetched via a prefetch operation.
    prefetched: bool,
}

impl Deref for CachedChangesetEdges {
    type Target = ChangesetEdges;

    fn deref(&self) -> &ChangesetEdges {
        &self.edges
    }
}

impl CachedChangesetEdges {
    fn fetched(edges: ChangesetEdges) -> Self {
        CachedChangesetEdges {
            edges,
            prefetched: false,
        }
    }

    fn prefetched(edges: ChangesetEdges) -> Self {
        CachedChangesetEdges {
            edges,
            prefetched: true,
        }
    }

    fn take(self) -> ChangesetEdges {
        if self.prefetched {
            STATS::hit.add_value(1);
        }
        self.edges
    }

    fn to_thrift(&self) -> thrift::ChangesetEdges {
        self.edges.to_thrift()
    }

    fn from_thrift(edges: thrift::ChangesetEdges) -> Result<Self> {
        Ok(Self {
            edges: ChangesetEdges::from_thrift(edges)?,
            prefetched: false,
        })
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
        if self.prefetch.is_include() {
            if justknobs::eval(
                "scm/mononoke:disable_commit_graph_memcache_for_prefetch",
                None,
                None,
            )
            .unwrap_or_default()
            {
                // If asked to prefetch, fetching from memcache is actually
                // slower, so don't perform memcache look-ups.
                &MemcacheHandler::Noop
            } else {
                &self.caching_storage.memcache
            }
        } else {
            &self.caching_storage.memcache
        }
    }

    fn cache_determinator(&self, _: &CachedChangesetEdges) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("commit_graph");
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
        let cs_ids: Vec<ChangesetId> = keys.iter().copied().collect();
        let entries = if self.required {
            self.caching_storage
                .storage
                .fetch_many_edges(self.ctx, &cs_ids, self.prefetch)
                .await?
        } else {
            self.caching_storage
                .storage
                .maybe_fetch_many_edges(self.ctx, &cs_ids, self.prefetch)
                .await?
        };
        if self.prefetch.is_include() {
            // We were asked to prefetch. We must separate out the prefetched
            // values from the fetched values as we may only return the
            // fetched values.
            let mut fetched = HashMap::new();
            let mut prefetched = HashMap::new();
            for (cs_id, edges) in entries {
                if keys.contains(&cs_id) {
                    fetched.insert(cs_id, CachedChangesetEdges::fetched(edges));
                } else {
                    prefetched.insert(cs_id, CachedChangesetEdges::prefetched(edges));
                }
            }
            if !prefetched.is_empty() {
                STATS::prefetched.add_value(prefetched.len() as i64);
                fill_cache(self, &prefetched).await;
            }
            STATS::fetched.add_value(fetched.len() as i64);
            Ok(fetched)
        } else {
            Ok(entries
                .into_iter()
                .map(|(cs_id, edges)| (cs_id, CachedChangesetEdges::fetched(edges)))
                .collect())
        }
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
        storage: Arc<dyn CommitGraphStorage>,
        cache_handler_factory: CacheHandlerFactory,
    ) -> Self {
        Self {
            repo_id: storage.repo_id(),
            storage,
            cachelib: cache_handler_factory.cachelib(),
            memcache: cache_handler_factory.memcache(),
            keygen: Self::keygen(),
        }
    }

    #[cfg(test)]
    pub fn mocked(storage: Arc<dyn CommitGraphStorage>) -> Self {
        Self::new(storage, CacheHandlerFactory::Mocked)
    }

    fn request<'a>(&'a self, ctx: &'a CoreContext, prefetch: Prefetch) -> CacheRequest<'a> {
        let prefetch = if justknobs::eval("scm/mononoke:disable_commit_graph_prefetch", None, None)
            .unwrap_or_default()
        {
            Prefetch::None
        } else {
            prefetch.include_hint()
        };
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
        let prefetch = if justknobs::eval("scm/mononoke:disable_commit_graph_prefetch", None, None)
            .unwrap_or_default()
        {
            Prefetch::None
        } else {
            prefetch.include_hint()
        };
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

    async fn fetch_edges(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<ChangesetEdges> {
        let mut found =
            get_or_fill(&self.request_required(ctx, Prefetch::None), hashset![cs_id]).await?;
        Ok(found
            .remove(&cs_id)
            .ok_or_else(|| anyhow!("Missing changeset from commit graph storage: {}", cs_id))?
            .take())
    }

    async fn maybe_fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        let mut found = get_or_fill(&self.request(ctx, Prefetch::None), hashset![cs_id]).await?;
        Ok(found.remove(&cs_id).map(CachedChangesetEdges::take))
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let cs_ids: HashSet<ChangesetId> = cs_ids.iter().copied().collect();
        let found = get_or_fill_chunked(
            &self.request_required(ctx, prefetch),
            cs_ids.clone(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?;
        Ok(found
            .into_iter()
            .map(|(cs_id, edges)| (cs_id, edges.take()))
            .collect())
    }

    async fn maybe_fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let cs_ids: HashSet<ChangesetId> = cs_ids.iter().copied().collect();
        let found = get_or_fill_chunked(
            &self.request(ctx, prefetch),
            cs_ids.clone(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?;
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

    async fn fetch_children(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        self.storage.fetch_children(ctx, cs_id).await
    }
}
