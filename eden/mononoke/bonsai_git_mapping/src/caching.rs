/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use ::sql::Transaction;
use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping_entry_thrift as thrift;
use bytes::Bytes;
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
use context::CoreContext;
use fbthrift::compact_protocol;
use memcache::KeyGen;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use stats::prelude::*;

use super::BonsaiGitMapping;
use super::BonsaiGitMappingEntry;
use super::BonsaisOrGitShas;
use crate::AddGitMappingErrorKind;

define_stats! {
    prefix = "mononoke.bonsai_git_mapping";
    memcache_hit: timeseries("memcache.hit"; Rate, Sum),
    memcache_miss: timeseries("memcache.miss"; Rate, Sum),
    memcache_internal_err: timeseries("memcache.internal_err"; Rate, Sum),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; Rate, Sum),
}

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGitMappingCacheEntry {
    pub repo_id: RepositoryId,
    pub git_sha1: GitSha1,
    pub bcs_id: ChangesetId,
}

impl BonsaiGitMappingCacheEntry {
    fn from_thrift(
        entry: bonsai_git_mapping_entry_thrift::BonsaiGitMappingCacheEntry,
    ) -> Result<Self> {
        Ok(Self {
            repo_id: RepositoryId::new(entry.repo_id.0),
            git_sha1: GitSha1::from_thrift(entry.git_sha1)?,
            bcs_id: ChangesetId::from_thrift(entry.bcs_id)?,
        })
    }

    fn into_thrift(self) -> bonsai_git_mapping_entry_thrift::BonsaiGitMappingCacheEntry {
        bonsai_git_mapping_entry_thrift::BonsaiGitMappingCacheEntry {
            repo_id: bonsai_git_mapping_entry_thrift::RepoId(self.repo_id.id()),
            git_sha1: self.git_sha1.into_thrift(),
            bcs_id: self.bcs_id.into_thrift(),
        }
    }

    pub fn new(repo_id: RepositoryId, git_sha1: GitSha1, bcs_id: ChangesetId) -> Self {
        BonsaiGitMappingCacheEntry {
            repo_id,
            git_sha1,
            bcs_id,
        }
    }

    pub fn into_entry(self, repo_id: RepositoryId) -> Result<BonsaiGitMappingEntry> {
        if self.repo_id == repo_id {
            Ok(BonsaiGitMappingEntry {
                git_sha1: self.git_sha1,
                bcs_id: self.bcs_id,
            })
        } else {
            Err(anyhow!(
                "Cache returned invalid entry: repo {} returned for query to repo {}",
                self.repo_id,
                repo_id
            ))
        }
    }

    pub fn from_entry(
        entry: BonsaiGitMappingEntry,
        repo_id: RepositoryId,
    ) -> BonsaiGitMappingCacheEntry {
        BonsaiGitMappingCacheEntry {
            repo_id,
            git_sha1: entry.git_sha1,
            bcs_id: entry.bcs_id,
        }
    }
}

/// Used for cache key generation
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum BonsaiOrGitSha {
    Bonsai(ChangesetId),
    GitSha1(GitSha1),
}

impl From<ChangesetId> for BonsaiOrGitSha {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaiOrGitSha::Bonsai(cs_id)
    }
}

impl From<GitSha1> for BonsaiOrGitSha {
    fn from(git_sha1: GitSha1) -> Self {
        BonsaiOrGitSha::GitSha1(git_sha1)
    }
}

pub struct CachingBonsaiGitMapping {
    mapping: Arc<dyn BonsaiGitMapping>,
    cachelib: CachelibHandler<BonsaiGitMappingCacheEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl CachingBonsaiGitMapping {
    pub fn new(
        mapping: Arc<dyn BonsaiGitMapping>,
        cache_handler_factory: CacheHandlerFactory,
    ) -> Result<Self> {
        Ok(Self {
            mapping,
            cachelib: cache_handler_factory.cachelib(),
            memcache: cache_handler_factory.memcache(),
            keygen: CachingBonsaiGitMapping::create_key_gen()?,
        })
    }

    pub fn new_test(mapping: Arc<dyn BonsaiGitMapping>) -> Self {
        Self {
            mapping,
            cachelib: CacheHandlerFactory::Mocked.cachelib(),
            memcache: CacheHandlerFactory::Mocked.memcache(),
            keygen: CachingBonsaiGitMapping::create_key_gen_test(),
        }
    }

    fn create_key_gen() -> Result<KeyGen> {
        let key_prefix = "scm.mononoke.bonsai_git_mapping";

        let sitever =
            justknobs::get_as::<u32>("scm/mononoke_memcache_sitevers:bonsai_git_mapping", None)?;

        Ok(KeyGen::new(key_prefix, thrift::MC_CODEVER as u32, sitever))
    }

    fn create_key_gen_test() -> KeyGen {
        let key_prefix = "scm.mononoke.bonsai_git_mapping_test";
        KeyGen::new(key_prefix, thrift::MC_CODEVER as u32, 0)
    }
}

fn memcache_deserialize(bytes: Bytes) -> McResult<BonsaiGitMappingCacheEntry> {
    let thrift_entry =
        compact_protocol::deserialize(bytes).map_err(|_| McErrorKind::Deserialization);
    thrift_entry.and_then(|entry| {
        BonsaiGitMappingCacheEntry::from_thrift(entry).map_err(|_| McErrorKind::Deserialization)
    })
}

fn memcache_serialize(entry: &BonsaiGitMappingCacheEntry) -> Bytes {
    compact_protocol::serialize(&entry.clone().into_thrift())
}

const CHUNK_SIZE: usize = 1000;
const PARALLEL_CHUNKS: usize = 1;

#[async_trait]
impl BonsaiGitMapping for CachingBonsaiGitMapping {
    fn repo_id(&self) -> RepositoryId {
        self.mapping.repo_id()
    }

    async fn add(
        &self,
        ctx: &CoreContext,
        entry: BonsaiGitMappingEntry,
    ) -> Result<(), AddGitMappingErrorKind> {
        self.mapping.add(ctx, entry).await
    }

    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        self.mapping.bulk_add(ctx, entries).await
    }

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind> {
        self.mapping
            .bulk_add_git_mapping_in_transaction(ctx, entries, transaction)
            .await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        cs: BonsaisOrGitShas,
    ) -> Result<Vec<BonsaiGitMappingEntry>, Error> {
        let cache_request = (ctx, self);
        let repo_id = self.repo_id();

        let cache_entry = match cs {
            BonsaisOrGitShas::Bonsai(cs_ids) => get_or_fill_chunked(
                &cache_request,
                cs_ids.into_iter().collect(),
                CHUNK_SIZE,
                PARALLEL_CHUNKS,
            )
            .await?
            .into_values()
            .map(|val| val.into_entry(repo_id))
            .collect::<Result<_>>()?,
            BonsaisOrGitShas::GitSha1(git_shas) => get_or_fill_chunked(
                &cache_request,
                git_shas.into_iter().collect(),
                CHUNK_SIZE,
                PARALLEL_CHUNKS,
            )
            .await?
            .into_values()
            .map(|val| val.into_entry(repo_id))
            .collect::<Result<_>>()?,
        };

        Ok(cache_entry)
    }

    /// Use caching for the ranges of one element, use slower path otherwise.
    async fn get_in_range(
        &self,
        ctx: &CoreContext,
        low: GitSha1,
        high: GitSha1,
        limit: usize,
    ) -> Result<Vec<GitSha1>, Error> {
        if low == high {
            let res = self.get(ctx, low.into()).await?;
            if res.is_empty() {
                return Ok(vec![]);
            } else {
                return Ok(vec![low]);
            }
        }

        self.mapping.get_in_range(ctx, low, high, limit).await
    }
}

fn get_cache_key(repo_id: RepositoryId, cs: &BonsaiOrGitSha) -> String {
    format!("{}.{:?}", repo_id.prefix(), cs)
}

impl MemcacheEntity for BonsaiGitMappingCacheEntry {
    fn serialize(&self) -> Bytes {
        memcache_serialize(self)
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        memcache_deserialize(bytes)
    }
}

type CacheRequest<'a> = (&'a CoreContext, &'a CachingBonsaiGitMapping);

impl EntityStore<BonsaiGitMappingCacheEntry> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiGitMappingCacheEntry> {
        let (_, mapping) = self;
        &mapping.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        let (_, mapping) = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, mapping) = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, _: &BonsaiGitMappingCacheEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_git_mapping");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, BonsaiGitMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, mapping) = self;
        get_cache_key(mapping.repo_id(), &BonsaiOrGitSha::Bonsai(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiGitMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .mapping
            .get(ctx, BonsaisOrGitShas::Bonsai(keys.into_iter().collect()))
            .await?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| (e.bcs_id, BonsaiGitMappingCacheEntry::from_entry(e, repo_id)))
                .collect(),
        )
    }
}

#[async_trait]
impl KeyedEntityStore<GitSha1, BonsaiGitMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &GitSha1) -> String {
        let (_, mapping) = self;
        get_cache_key(mapping.repo_id(), &BonsaiOrGitSha::GitSha1(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<GitSha1>,
    ) -> Result<HashMap<GitSha1, BonsaiGitMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .mapping
            .get(ctx, BonsaisOrGitShas::GitSha1(keys.into_iter().collect()))
            .await?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| {
                    (
                        e.git_sha1,
                        BonsaiGitMappingCacheEntry::from_entry(e, repo_id),
                    )
                })
                .collect(),
        )
    }
}
