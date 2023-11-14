/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Result as IoResult;
use std::io::Write;
use std::ops::Deref;
use std::sync::Arc;

use abomonation::Abomonation;
use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use caching_ext::fill_cachelib;
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
use commit_graph_types::storage::FetchedChangesetEdges;
use commit_graph_types::storage::Prefetch;
use commit_graph_types::storage::PrefetchEdge;
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
    memcache_hit: timeseries("memcache_hit"; Rate, Sum),
    fetched: timeseries("fetched"; Rate, Sum),
    prefetched: timeseries("prefetched"; Rate, Sum),
    memcache_fetched: timeseries("memcache_fetched"; Rate, Sum),
    memcache_prefetched: timeseries("memcache_prefetched"; Rate, Sum),
}

/// Size of chunk when fetching from the backing store
const CHUNK_SIZE: usize = 1000;

/// Number of chunks to fetch in parallel
const PARALLEL_CHUNKS: usize = 2;

/// Caching Commit Graph Storage
pub struct CachingCommitGraphStorage {
    storage: Arc<dyn CommitGraphStorage>,
    cachelib: CachelibHandler<CachedPrefetchedChangesetEdges>,
    memcache: MemcacheHandler,
    keygen_single: KeyGen,
    keygen_prefetch_p1_linear: KeyGen,
    keygen_prefetch_skip_tree: KeyGen,
    repo_id: RepositoryId,
}

struct CacheRequest<'a> {
    ctx: &'a CoreContext,
    caching_storage: &'a CachingCommitGraphStorage,
    prefetch: Prefetch,
    required: bool,
    memcache_prefetch: bool,
}

/// Origin of a value in the cachelib cache.
#[derive(Copy, Clone, Debug, Abomonation)]
pub enum CacheOrigin {
    /// This cached value originated from a direct fetch.
    Fetched,

    /// This cached value originated from a prefetch.
    Prefetched,

    /// This cached value originated from a memcache hit of a direct fetch.
    MemcacheFetched,

    /// This cached value originated from a memcache hit of a prefetch.
    MemcachePrefetched,
}

#[derive(Clone, Debug, Abomonation)]
/// A cached copy of changeset edges
///
/// This structure contains what is stored in the in-memory cache (cachelib).
pub struct CachedChangesetEdges {
    /// The cached edges.
    edges: ChangesetEdges,

    /// Whether these edges were originally fetched directly, or from some
    /// prefetch operation.
    cache_origin: CacheOrigin,
}

#[derive(Clone, Debug)]
/// A cached copy of changeset edges, along with the edges that were prefetched alongside them.
///
/// This structure contains what is stored in the shared remote cache (memcache).  When serialized
/// for the in-memory cache (cachelib), the prefetched edges are omitted.
pub struct CachedPrefetchedChangesetEdges {
    /// The cached edges for the changeset referenced by the cache key.
    inner: CachedChangesetEdges,

    /// Edges that were prefetched alongside this changeset.  These are only stored in memcache,
    /// which means that we can retrieve more than one edge at a time when the cache hits.  The
    /// prefetch parameter may not exactly match the value we are looking for when prefetching
    /// later on, but this should not matter.  If it is too small, when we encounter something
    /// that is missing, we will fetch from that point, which is still better than nothing.
    prefetched_edges: HashMap<ChangesetId, ChangesetEdges>,
}

impl Abomonation for CachedPrefetchedChangesetEdges {
    #[inline(always)]
    unsafe fn entomb<W: Write>(&self, write: &mut W) -> IoResult<()> {
        // SAFETY: This implementation matches the proc-macro-generated version but with `prefetched_edges` excluded, and matches the exhume method below.
        self.inner.entomb(write)?;
        // We deliberately do not entomb the contents of `prefetched_edges`.  It will be re-initialized when exhumed.
        Ok(())
    }

    #[inline(always)]
    unsafe fn exhume<'a, 'b>(&'a mut self, bytes: &'b mut [u8]) -> Option<&'b mut [u8]> {
        // SAFETY: This implementation matches the proc-macro-generated version but with `prefetched_edges` re-initialized, and matches the entomb method above.
        let bytes = self.inner.exhume(bytes)?;
        // Re-initialize `prefetched_edges` as its contents were not entombed.
        std::ptr::write(&mut self.prefetched_edges, HashMap::new());
        Some(bytes)
    }

    #[inline(always)]
    fn extent(&self) -> usize {
        self.inner.extent()
    }
}

impl Deref for CachedPrefetchedChangesetEdges {
    type Target = ChangesetEdges;

    fn deref(&self) -> &ChangesetEdges {
        &self.inner.edges
    }
}

impl CachedPrefetchedChangesetEdges {
    fn fetched(edges: ChangesetEdges) -> Self {
        CachedPrefetchedChangesetEdges {
            inner: CachedChangesetEdges {
                edges,
                cache_origin: CacheOrigin::Fetched,
            },
            prefetched_edges: HashMap::new(),
        }
    }

    fn prefetched(edges: ChangesetEdges) -> Self {
        CachedPrefetchedChangesetEdges {
            inner: CachedChangesetEdges {
                edges,
                cache_origin: CacheOrigin::Prefetched,
            },
            prefetched_edges: HashMap::new(),
        }
    }

    fn memcache_prefetched(edges: ChangesetEdges) -> Self {
        CachedPrefetchedChangesetEdges {
            inner: CachedChangesetEdges {
                edges,
                cache_origin: CacheOrigin::MemcachePrefetched,
            },
            prefetched_edges: HashMap::new(),
        }
    }

    fn take(self) -> ChangesetEdges {
        match self.inner.cache_origin {
            CacheOrigin::Fetched | CacheOrigin::MemcacheFetched => {}
            CacheOrigin::Prefetched => STATS::hit.add_value(1),
            CacheOrigin::MemcachePrefetched => STATS::memcache_hit.add_value(1),
        }
        self.inner.edges
    }

    fn to_thrift(&self) -> thrift::CachedChangesetEdges {
        let prefetched_edges = Some(
            self.prefetched_edges
                .values()
                .map(ChangesetEdges::to_thrift)
                .collect::<Vec<_>>(),
        )
        .filter(|prefetched_edges| !prefetched_edges.is_empty());

        thrift::CachedChangesetEdges {
            edges: self.inner.edges.to_thrift(),
            prefetched_edges,
        }
    }

    fn from_thrift(cached_edges: thrift::CachedChangesetEdges) -> Result<Self> {
        let inner = CachedChangesetEdges {
            edges: ChangesetEdges::from_thrift(cached_edges.edges)?,
            cache_origin: CacheOrigin::MemcacheFetched,
        };
        let mut prefetched_edges = HashMap::new();
        if let Some(cached_prefetched_edges) = cached_edges.prefetched_edges {
            for edges in cached_prefetched_edges {
                prefetched_edges.insert(
                    ChangesetId::from_thrift(edges.node.cs_id.clone())?,
                    ChangesetEdges::from_thrift(edges)?,
                );
            }
        }
        Ok(CachedPrefetchedChangesetEdges {
            inner,
            prefetched_edges,
        })
    }
}

impl MemcacheEntity for CachedPrefetchedChangesetEdges {
    fn serialize(&self) -> Bytes {
        compact_protocol::serialize(&self.to_thrift())
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        compact_protocol::deserialize(bytes)
            .and_then(CachedPrefetchedChangesetEdges::from_thrift)
            .map_err(|_| McErrorKind::Deserialization)
    }
}

impl EntityStore<CachedPrefetchedChangesetEdges> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<CachedPrefetchedChangesetEdges> {
        &self.caching_storage.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        if self.memcache_prefetch {
            match self.prefetch.target_edge() {
                Some(PrefetchEdge::FirstParent) => &self.caching_storage.keygen_prefetch_p1_linear,
                Some(PrefetchEdge::SkipTreeSkewAncestor) => {
                    &self.caching_storage.keygen_prefetch_skip_tree
                }
                None => &self.caching_storage.keygen_single,
            }
        } else {
            &self.caching_storage.keygen_single
        }
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

    fn cache_determinator(&self, _: &CachedPrefetchedChangesetEdges) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("commit_graph");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, CachedPrefetchedChangesetEdges> for CacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        self.caching_storage.cache_key(cs_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, CachedPrefetchedChangesetEdges>> {
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
            // fetched values, and need to attach the prefetched values to
            // the fetched values for memcache.
            let mut fetched = HashMap::new();
            let mut prefetched = HashMap::new();
            // Also collect the prefetched values indexed by the original commit
            // they were prefetched for.  This is used to populate memcache
            // prefetch values.
            let mut prefetched_by_origin = HashMap::new();
            for (cs_id, edges) in entries {
                if keys.contains(&cs_id) {
                    fetched.insert(cs_id, CachedPrefetchedChangesetEdges::fetched(edges.into()));
                } else {
                    prefetched.insert(
                        cs_id,
                        CachedPrefetchedChangesetEdges::prefetched(edges.clone().into()),
                    );
                    if let Some(origin_cs_id) = edges.prefetched_for() {
                        prefetched_by_origin
                            .entry(origin_cs_id)
                            .or_insert_with(HashMap::new)
                            .insert(cs_id, edges.into());
                    }
                }
            }
            if !prefetched.is_empty() {
                // Fill cachelib with all the additionally prefetched values.
                STATS::prefetched.add_value(prefetched.len() as i64);
                fill_cachelib(self, &prefetched);
            }
            if self.memcache_prefetch {
                // Add all prefetched values to their fetched origin.  This will
                // be stored in memcache.
                for (origin_csid, prefetched_edges) in prefetched_by_origin {
                    if let Some(edges) = fetched.get_mut(&origin_csid) {
                        edges.prefetched_edges.extend(prefetched_edges);
                    }
                }
            }
            STATS::fetched.add_value(fetched.len() as i64);
            Ok(fetched)
        } else {
            Ok(entries
                .into_iter()
                .map(|(cs_id, edges)| {
                    (cs_id, CachedPrefetchedChangesetEdges::fetched(edges.into()))
                })
                .collect())
        }
    }

    fn on_memcache_hits<'a>(
        &self,
        values: impl IntoIterator<Item = (&'a ChangesetId, &'a CachedPrefetchedChangesetEdges)>,
    ) {
        let mut fetched = 0;
        for (_cs_id, edges) in values {
            fetched += 1;
            if !edges.prefetched_edges.is_empty() {
                let prefetched_edges = edges
                    .prefetched_edges
                    .iter()
                    .map(|(k, v)| {
                        let edges = CachedPrefetchedChangesetEdges::memcache_prefetched(v.clone());
                        (*k, edges)
                    })
                    .collect::<HashMap<_, _>>();
                STATS::memcache_prefetched.add_value(prefetched_edges.len() as i64);
                fill_cachelib(self, &prefetched_edges);
            }
        }
        STATS::memcache_fetched.add_value(fetched);
    }
}

impl CachingCommitGraphStorage {
    fn keygen(key_prefix: &'static str) -> KeyGen {
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
            keygen_single: Self::keygen("scm.mononoke.commitgraph"),
            keygen_prefetch_p1_linear: Self::keygen("scm.mononoke.commitgraph.p1"),
            keygen_prefetch_skip_tree: Self::keygen("scm.mononoke.commitgraph.sk"),
        }
    }

    #[cfg(test)]
    pub fn mocked(storage: Arc<dyn CommitGraphStorage>) -> Self {
        Self::new(storage, CacheHandlerFactory::Mocked)
    }

    /// Determine prefetch parameters for this request based on the prefetch
    /// requested by the user and current rollout values.
    fn request_prefetch_params(prefetch: Prefetch) -> (Prefetch, bool) {
        let prefetch = if justknobs::eval("scm/mononoke:disable_commit_graph_prefetch", None, None)
            .unwrap_or_default()
        {
            Prefetch::None
        } else {
            prefetch.include_hint()
        };
        let memcache_prefetch = justknobs::eval(
            "scm/mononoke:commit_graph_prefetch_store_in_memcache",
            None,
            None,
        )
        .unwrap_or_default();
        (prefetch, memcache_prefetch)
    }

    fn request<'a>(&'a self, ctx: &'a CoreContext, prefetch: Prefetch) -> CacheRequest<'a> {
        let (prefetch, memcache_prefetch) = Self::request_prefetch_params(prefetch);
        CacheRequest {
            ctx,
            caching_storage: self,
            prefetch,
            memcache_prefetch,
            required: false,
        }
    }

    fn request_required<'a>(
        &'a self,
        ctx: &'a CoreContext,
        prefetch: Prefetch,
    ) -> CacheRequest<'a> {
        let (prefetch, memcache_prefetch) = Self::request_prefetch_params(prefetch);
        CacheRequest {
            ctx,
            caching_storage: self,
            prefetch,
            memcache_prefetch,
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
        Ok(found
            .remove(&cs_id)
            .map(CachedPrefetchedChangesetEdges::take))
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
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
            .map(|(cs_id, edges)| (cs_id, edges.take().into()))
            .collect())
    }

    async fn maybe_fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
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
            .map(|(cs_id, edges)| (cs_id, edges.take().into()))
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
