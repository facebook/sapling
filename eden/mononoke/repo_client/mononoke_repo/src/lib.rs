/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Context, Error};
use blobrepo::BlobRepo;
use blobstore_factory::ReadOnlyStorage;
use cacheblob::LeaseOps;
use fbinit::FacebookInit;
use futures::future::{FutureExt, TryFutureExt};
use futures_01_ext::{BoxFuture, FutureExt as _};
use getbundle_response::SessionLfsParams;
use hooks::HookManager;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::{
    BookmarkAttrs, InfinitepushParams, MetadataDatabaseConfig, PushParams, PushrebaseParams,
    RepoReadOnly,
};
use mononoke_api::Repo;
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use rand::Rng;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_blobstore::RepoBlobstore;
use repo_read_write_status::RepoReadWriteFetcher;
use reverse_filler_queue::ReverseFillerQueue;
use reverse_filler_queue::SqlReverseFillerQueue;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use std::fmt::{self, Debug};
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use streaming_clone::SqlStreamingChunksFetcher;
use warm_bookmarks_cache::BookmarksCache;

#[derive(Clone)]
pub struct SqlStreamingCloneConfig {
    pub blobstore: RepoBlobstore,
    pub fetcher: SqlStreamingChunksFetcher,
    pub repoid: RepositoryId,
}

#[derive(Clone)]
pub struct MononokeRepo {
    repo: Arc<Repo>,
    bookmark_attrs: BookmarkAttrs,
    streaming_clone: SqlStreamingCloneConfig,
    #[allow(dead_code)]
    mutable_counters: Arc<dyn MutableCounters>,
    // Reverse filler queue for recording accepted infinitepush bundles
    // This field is `None` if we don't want recording to happen
    maybe_reverse_filler_queue: Option<Arc<dyn ReverseFillerQueue>>,
}

impl MononokeRepo {
    pub async fn new(
        fb: FacebookInit,
        repo: Arc<Repo>,
        mysql_options: &MysqlOptions,
        readonly_storage: ReadOnlyStorage,
    ) -> Result<Self, Error> {
        let storage_config = &repo.config().storage_config;

        let mutable_counters = Arc::new(
            SqlMutableCounters::with_metadata_database_config(
                fb,
                &storage_config.metadata,
                mysql_options,
                readonly_storage.0,
            )
            .context("Failed to open SqlMutableCounters")?,
        );

        let streaming_clone = streaming_clone(
            fb,
            repo.blob_repo(),
            &storage_config.metadata,
            mysql_options,
            repo.repoid(),
            readonly_storage.0,
        )?;

        let maybe_reverse_filler_queue = {
            let record_infinitepush_writes: bool =
                repo.config().infinitepush.populate_reverse_filler_queue
                    && repo.config().infinitepush.allow_writes;

            if record_infinitepush_writes {
                let reverse_filler_queue = SqlReverseFillerQueue::with_metadata_database_config(
                    fb,
                    &storage_config.metadata,
                    mysql_options,
                    readonly_storage.0,
                )?;

                let reverse_filler_queue: Arc<dyn ReverseFillerQueue> =
                    Arc::new(reverse_filler_queue);
                Some(reverse_filler_queue)
            } else {
                None
            }
        };

        Self::new_from_parts(
            fb,
            repo,
            streaming_clone,
            mutable_counters,
            maybe_reverse_filler_queue,
        )
        .await
    }

    pub async fn new_from_parts(
        fb: FacebookInit,
        repo: Arc<Repo>,
        streaming_clone: SqlStreamingCloneConfig,
        mutable_counters: Arc<dyn MutableCounters>,
        maybe_reverse_filler_queue: Option<Arc<dyn ReverseFillerQueue>>,
    ) -> Result<Self, Error> {
        // TODO: Update Metaconfig so we just have this in config:
        let bookmark_attrs = BookmarkAttrs::new(fb, repo.config().bookmarks.clone()).await?;

        Ok(Self {
            repo,
            streaming_clone,
            mutable_counters,
            maybe_reverse_filler_queue,
            bookmark_attrs,
        })
    }

    pub fn blobrepo(&self) -> &BlobRepo {
        &self.repo.blob_repo()
    }

    pub fn pushrebase_params(&self) -> &PushrebaseParams {
        &self.repo.config().pushrebase
    }

    pub fn hipster_acl(&self) -> &Option<String> {
        &self.repo.config().hipster_acl
    }

    pub fn push_params(&self) -> &PushParams {
        &self.repo.config().push
    }

    pub fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
        self.repo.config().repo_client_use_warm_bookmarks_cache
    }

    pub fn bookmark_attrs(&self) -> BookmarkAttrs {
        self.bookmark_attrs.clone()
    }

    pub fn hook_manager(&self) -> Arc<HookManager> {
        self.repo.hook_manager().clone()
    }

    pub fn streaming_clone(&self) -> &SqlStreamingCloneConfig {
        &self.streaming_clone
    }

    pub fn maybe_reverse_filler_queue(&self) -> Option<&dyn ReverseFillerQueue> {
        self.maybe_reverse_filler_queue.as_deref()
    }

    pub fn force_lfs_if_threshold_set(&self) -> SessionLfsParams {
        SessionLfsParams {
            threshold: self.repo.config().lfs.threshold,
        }
    }

    pub fn lfs_params(&self, client_hostname: Option<&str>) -> SessionLfsParams {
        let percentage = self.repo.config().lfs.rollout_percentage;

        let allowed = match client_hostname {
            Some(client_hostname) => {
                let mut hasher = DefaultHasher::new();
                client_hostname.hash(&mut hasher);
                hasher.finish() % 100 < percentage.into()
            }
            None => {
                // Randomize in case source hostname is not set to avoid
                // sudden jumps in traffic
                rand::thread_rng().gen_ratio(percentage.into(), 100)
            }
        };

        if allowed {
            SessionLfsParams {
                threshold: self.repo.config().lfs.threshold,
            }
        } else {
            SessionLfsParams { threshold: None }
        }
    }

    pub fn reponame(&self) -> &String {
        &self.repo.name()
    }

    pub fn repoid(&self) -> RepositoryId {
        self.repo.repoid()
    }

    pub fn readonly(&self) -> BoxFuture<RepoReadOnly, Error> {
        let repo = self.repo.clone();
        async move { repo.readonly_fetcher().readonly().await }
            .boxed()
            .compat()
            .boxify()
    }

    pub fn readonly_fetcher(&self) -> &RepoReadWriteFetcher {
        &self.repo.readonly_fetcher()
    }

    pub fn infinitepush(&self) -> &InfinitepushParams {
        &self.repo.config().infinitepush
    }

    pub fn list_keys_patterns_max(&self) -> u64 {
        self.repo.config().list_keys_patterns_max
    }

    pub fn lca_hint(&self) -> Arc<dyn LeastCommonAncestorsHint> {
        self.repo.skiplist_index().clone()
    }

    pub fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache> {
        self.repo.warm_bookmarks_cache()
    }

    pub fn live_commit_sync_config(&self) -> Arc<dyn LiveCommitSyncConfig> {
        self.repo.live_commit_sync_config()
    }

    pub fn x_repo_sync_lease(&self) -> &Arc<dyn LeaseOps> {
        self.repo.x_repo_sync_lease()
    }
}

fn streaming_clone(
    fb: FacebookInit,
    blobrepo: &BlobRepo,
    metadata_db_config: &MetadataDatabaseConfig,
    mysql_options: &MysqlOptions,
    repoid: RepositoryId,
    readonly_storage: bool,
) -> Result<SqlStreamingCloneConfig, Error> {
    let fetcher = SqlStreamingChunksFetcher::with_metadata_database_config(
        fb,
        metadata_db_config,
        mysql_options,
        readonly_storage,
    )
    .context("Failed to open SqlStreamingChunksFetcher")?;

    Ok(SqlStreamingCloneConfig {
        fetcher,
        blobstore: blobrepo.get_blobstore(),
        repoid,
    })
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MononokeRepo({:#?})", self.repo.repoid())
    }
}
