/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
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
use caching_ext::get_or_fill_chunked;
use context::CoreContext;
use fbthrift::compact_protocol;
use itertools::Itertools;
use memcache::KeyGen;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use quickcheck::Arbitrary;
use synced_commit_mapping_thrift as thrift;

use crate::EquivalentWorkingCopyEntry;
use crate::FetchedMappingEntry;
use crate::SyncedCommitMapping;
use crate::SyncedCommitMappingEntry;
use crate::WorkingCopyEquivalence;
use crate::sql::BATCH_TARGET_REPOS_KNOB;
use crate::types::get_maybe_stale_many_targets_serially;

/// Caching layer for SyncedCommitMapping. The cache works as a map from
/// `(source_repo_id, target_repo_id, bcs_id)` to a list mappings. Caching
/// is only performed when the mapping is not empty i.e. there's no negative
/// caching.
pub struct CachingSyncedCommitMapping {
    inner_mapping: Arc<dyn SyncedCommitMapping>,
    cachelib: CachelibHandler<CacheEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl CachingSyncedCommitMapping {
    pub fn new(
        inner_mapping: Arc<dyn SyncedCommitMapping>,
        cache_handler_factory: CacheHandlerFactory,
    ) -> Result<Self, Error> {
        Ok(Self {
            inner_mapping,
            cachelib: cache_handler_factory.cachelib(),
            memcache: cache_handler_factory.memcache(),
            keygen: Self::keygen()?,
        })
    }

    fn keygen() -> Result<KeyGen, Error> {
        let key_prefix = "scm.mononoke.syncedcommitmapping";
        let sitever =
            justknobs::get_as::<u32>("scm/mononoke_memcache_sitevers:synced_commit_mapping", None);
        Ok(KeyGen::new(key_prefix, thrift::MC_CODEVER as u32, sitever))
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[derive(bincode::Encode, bincode::Decode)]
struct CacheEntry {
    mapping_entries: Vec<FetchedMappingEntry>,
}

impl CacheEntry {
    fn to_thrift(&self) -> thrift::CacheEntry {
        thrift::CacheEntry {
            mapping_entries: self
                .mapping_entries
                .iter()
                .map(FetchedMappingEntry::to_thrift)
                .collect(),
        }
    }
    fn from_thrift(cache_entry: thrift::CacheEntry) -> Result<Self, Error> {
        let mapping_entries = cache_entry
            .mapping_entries
            .into_iter()
            .map(FetchedMappingEntry::from_thrift)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CacheEntry { mapping_entries })
    }
}

impl MemcacheEntity for CacheEntry {
    fn serialize(&self) -> Bytes {
        compact_protocol::serialize(self.to_thrift())
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        compact_protocol::deserialize(bytes)
            .and_then(CacheEntry::from_thrift)
            .map_err(|_| McErrorKind::Deserialization)
    }
}

impl Arbitrary for CacheEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mapping_entries: Vec<FetchedMappingEntry> = Arbitrary::arbitrary(g);
        CacheEntry { mapping_entries }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
    bcs_id: ChangesetId,
}

impl CacheKey {
    fn format_cache_key(&self) -> String {
        format!(
            "{}.{}.{}",
            self.source_repo_id, self.target_repo_id, self.bcs_id
        )
    }
}

/// How `get_from_db` should resolve the keys it is handed.
enum FetchMode {
    /// Many changesets, one target repo.
    PerTargetRepo { maybe_stale: bool },
    /// One changeset, many target repos. Always replica-only.
    ///
    /// Carries the changeset the keys are all for, so that `get_from_db` can
    /// take the target repos straight off the keys instead of regrouping them.
    ManyTargetRepos {
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
    },
}

struct CacheRequest<'a> {
    ctx: &'a CoreContext,
    mapping: &'a CachingSyncedCommitMapping,
    fetch_mode: FetchMode,
}

const CHUNK_SIZE: usize = 1000;
const PARALLEL_CHUNKS: usize = 1;

impl EntityStore<CacheEntry> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<CacheEntry> {
        let CacheRequest { mapping, .. } = self;
        &mapping.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        let CacheRequest { mapping, .. } = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let CacheRequest { mapping, .. } = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, entry: &CacheEntry) -> CacheDisposition {
        if entry.mapping_entries.is_empty() {
            CacheDisposition::Ignore
        } else {
            CacheDisposition::Cache(CacheTtl::NoTtl)
        }
    }

    caching_ext::impl_singleton_stats!("synced_commit_mapping");
}

#[async_trait]
impl KeyedEntityStore<CacheKey, CacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &CacheKey) -> String {
        key.format_cache_key()
    }

    async fn get_from_db(
        &self,
        keys: HashSet<CacheKey>,
    ) -> Result<HashMap<CacheKey, CacheEntry>, Error> {
        let CacheRequest {
            ctx,
            mapping,
            fetch_mode,
        } = self;

        let maybe_stale = match fetch_mode {
            FetchMode::PerTargetRepo { maybe_stale } => *maybe_stale,
            FetchMode::ManyTargetRepos {
                source_repo_id,
                bcs_id,
            } => {
                let (source_repo_id, bcs_id) = (*source_repo_id, *bcs_id);
                let target_repo_ids: Vec<_> =
                    keys.into_iter().map(|key| key.target_repo_id).collect();

                let by_target_repo = mapping
                    .inner_mapping
                    .get_maybe_stale_many_targets(ctx, source_repo_id, bcs_id, &target_repo_ids)
                    .await?;

                return Ok(by_target_repo
                    .into_iter()
                    .map(|(target_repo_id, mapping_entries)| {
                        (
                            CacheKey {
                                source_repo_id,
                                target_repo_id,
                                bcs_id,
                            },
                            CacheEntry { mapping_entries },
                        )
                    })
                    .collect());
            }
        };

        let keys = keys
            .into_iter()
            .map(|key| ((key.source_repo_id, key.target_repo_id), key.bcs_id))
            .into_group_map();

        let mut res = HashMap::new();

        for ((source_repo_id, target_repo_id), bcs_ids) in keys {
            let entries = if maybe_stale {
                mapping
                    .inner_mapping
                    .get_many_maybe_stale(ctx, source_repo_id, target_repo_id, &bcs_ids)
                    .await?
            } else {
                mapping
                    .inner_mapping
                    .get_many(ctx, source_repo_id, target_repo_id, &bcs_ids)
                    .await?
            };

            for (bcs_id, entries) in entries {
                res.insert(
                    CacheKey {
                        source_repo_id,
                        target_repo_id,
                        bcs_id,
                    },
                    CacheEntry {
                        mapping_entries: entries,
                    },
                );
            }
        }

        Ok(res)
    }
}

#[async_trait]
impl SyncedCommitMapping for CachingSyncedCommitMapping {
    async fn add(&self, ctx: &CoreContext, entry: SyncedCommitMappingEntry) -> Result<bool, Error> {
        self.inner_mapping.add(ctx, entry).await
    }

    async fn add_bulk(
        &self,
        ctx: &CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> Result<u64, Error> {
        self.inner_mapping.add_bulk(ctx, entries).await
    }

    async fn get_many(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        bcs_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Vec<FetchedMappingEntry>>, Error> {
        let cache_request = CacheRequest {
            ctx,
            mapping: self,
            fetch_mode: FetchMode::PerTargetRepo { maybe_stale: false },
        };

        let entries = get_or_fill_chunked(
            &cache_request,
            bcs_ids
                .iter()
                .map(|bcs_id| CacheKey {
                    source_repo_id,
                    target_repo_id,
                    bcs_id: *bcs_id,
                })
                .collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?
        .into_iter()
        .map(|(key, entry)| (key.bcs_id, entry.mapping_entries))
        .collect();

        Ok(entries)
    }

    async fn get_many_maybe_stale(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        bcs_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Vec<FetchedMappingEntry>>, Error> {
        let cache_request = CacheRequest {
            ctx,
            mapping: self,
            fetch_mode: FetchMode::PerTargetRepo { maybe_stale: true },
        };

        let entries = get_or_fill_chunked(
            &cache_request,
            bcs_ids
                .iter()
                .map(|bcs_id| CacheKey {
                    source_repo_id,
                    target_repo_id,
                    bcs_id: *bcs_id,
                })
                .collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?
        .into_iter()
        .map(|(key, entry)| (key.bcs_id, entry.mapping_entries))
        .collect();

        Ok(entries)
    }

    async fn get_maybe_stale_many_targets(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_ids: &[RepositoryId],
    ) -> Result<HashMap<RepositoryId, Vec<FetchedMappingEntry>>, Error> {
        // Gated here as well as in the inner mapping, because the batched cache
        // lookup below is itself a behaviour change: off has to restore the
        // per-target-repo lookups, not just the query their misses turn into.
        if !justknobs::eval(BATCH_TARGET_REPOS_KNOB, None, None) {
            return get_maybe_stale_many_targets_serially(
                self,
                ctx,
                source_repo_id,
                bcs_id,
                target_repo_ids,
            )
            .await;
        }

        let cache_request = CacheRequest {
            ctx,
            mapping: self,
            fetch_mode: FetchMode::ManyTargetRepos {
                source_repo_id,
                bcs_id,
            },
        };

        let entries = get_or_fill_chunked(
            &cache_request,
            target_repo_ids
                .iter()
                .map(|target_repo_id| CacheKey {
                    source_repo_id,
                    target_repo_id: *target_repo_id,
                    bcs_id,
                })
                .collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await?
        .into_iter()
        .map(|(key, entry)| (key.target_repo_id, entry.mapping_entries))
        .collect();

        Ok(entries)
    }

    async fn insert_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> Result<bool, Error> {
        self.inner_mapping
            .insert_equivalent_working_copy(ctx, entry)
            .await
    }

    async fn overwrite_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> Result<bool, Error> {
        self.inner_mapping
            .overwrite_equivalent_working_copy(ctx, entry)
            .await
    }

    async fn get_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        source_bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> Result<Option<WorkingCopyEquivalence>, Error> {
        self.inner_mapping
            .get_equivalent_working_copy(ctx, source_repo_id, source_bcs_id, target_repo_id)
            .await
    }

    async fn insert_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
        version: &CommitSyncConfigVersion,
    ) -> Result<bool, Error> {
        self.inner_mapping
            .insert_large_repo_commit_version(ctx, large_repo_id, large_repo_cs_id, version)
            .await
    }

    async fn overwrite_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
        version: &CommitSyncConfigVersion,
    ) -> Result<bool, Error> {
        self.inner_mapping
            .overwrite_large_repo_commit_version(ctx, large_repo_id, large_repo_cs_id, version)
            .await
    }

    async fn get_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
    ) -> Result<Option<CommitSyncConfigVersion>, Error> {
        self.inner_mapping
            .get_large_repo_commit_version(ctx, large_repo_id, large_repo_cs_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn cache_entry_thrift_roundtrip(entry: CacheEntry) -> bool {
            let thrift_entry = entry.to_thrift();
            let roundtripped_entry = CacheEntry::from_thrift(thrift_entry)
                .expect("converting a valid Thrift structure should always work");
            entry == roundtripped_entry
        }
    }
}
