/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, BlobstoreOptions, Caching, ReadOnlyStorage};
use context::CoreContext;
use futures_preview::{compat::Future01CompatExt, future};
use hooks::HookManager;
use metaconfig_types::{MetadataDBConfig, RepoConfig};
use mutable_counters::SqlMutableCounters;
use phases::{CachingPhases, Phases, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use repo_read_write_status::{RepoReadWriteFetcher, SqlRepoReadWriteStatus};
use skiplist::fetch_skiplist_index;
use sql_ext::MysqlOptions;
use sql_ext::SqlConstructors;
use std::sync::Arc;

use crate::{streaming_clone, MononokeRepo};

pub struct MononokeRepoBuilder {
    ctx: CoreContext,
    name: String,
    repo: BlobRepo,
    config: RepoConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
}

impl MononokeRepoBuilder {
    pub async fn prepare(
        ctx: CoreContext,
        name: String,
        config: RepoConfig,
        mysql_options: MysqlOptions,
        caching: Caching,
        scuba_censored_table: Option<String>,
        readonly_storage: ReadOnlyStorage,
        blobstore_options: BlobstoreOptions,
    ) -> Result<MononokeRepoBuilder, Error> {
        let repo = open_blobrepo(
            ctx.fb,
            config.storage_config.clone(),
            config.repoid,
            mysql_options,
            caching,
            config.bookmarks_cache_ttl,
            config.redaction,
            scuba_censored_table.clone(),
            config.filestore.clone(),
            readonly_storage,
            blobstore_options.clone(),
            ctx.logger().clone(),
        )
        .compat()
        .await?;

        Ok(Self {
            ctx,
            name,
            repo,
            config,
            mysql_options,
            readonly_storage,
        })
    }

    pub async fn finalize(self, hook_manager: Arc<HookManager>) -> Result<MononokeRepo, Error> {
        let Self {
            ctx,
            name,
            repo,
            config,
            mysql_options,
            readonly_storage,
            ..
        } = self;

        let RepoConfig {
            storage_config,
            repoid,
            write_lock_db_address,
            pushrebase,
            bookmarks,
            lfs,
            infinitepush,
            list_keys_patterns_max,
            readonly,
            skiplist_index_blobstore_key,
            ..
        } = config;

        let streaming_clone = async {
            if let Some(db_address) = storage_config.dbconfig.get_db_address() {
                let r = streaming_clone(
                    ctx.fb,
                    repo.clone(),
                    db_address,
                    mysql_options,
                    repoid,
                    readonly_storage.0,
                )
                .compat()
                .await?;
                Ok(Some(r))
            } else {
                Ok(None)
            }
        };

        let sql_read_write_status = async {
            if let Some(addr) = write_lock_db_address {
                let r = SqlRepoReadWriteStatus::with_xdb(
                    ctx.fb,
                    addr,
                    mysql_options,
                    readonly_storage.0,
                )
                .compat()
                .await?;
                Ok(Some(r))
            } else {
                Ok(None)
            }
        };

        let phases_hint = SqlPhases::with_db_config(
            ctx.fb,
            &storage_config.dbconfig,
            mysql_options,
            readonly_storage.0,
        )
        .compat();

        let mutable_counters = SqlMutableCounters::with_db_config(
            ctx.fb,
            &storage_config.dbconfig,
            mysql_options,
            readonly_storage.0,
        )
        .compat();

        let skiplist = fetch_skiplist_index(
            ctx.clone(),
            skiplist_index_blobstore_key,
            repo.get_blobstore().boxed(),
        )
        .compat();

        let (streaming_clone, sql_read_write_status, phases_hint, mutable_counters, skiplist) =
            future::try_join5(
                streaming_clone,
                sql_read_write_status,
                phases_hint,
                mutable_counters,
                skiplist,
            )
            .await?;

        let read_write_fetcher =
            RepoReadWriteFetcher::new(sql_read_write_status, readonly, name.clone());

        let phases_hint: Arc<dyn Phases> =
            if let MetadataDBConfig::Mysql { .. } = storage_config.dbconfig {
                Arc::new(CachingPhases::new(ctx.fb, Arc::new(phases_hint)))
            } else {
                Arc::new(phases_hint)
            };

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = skiplist;

        let repo = MononokeRepo::new(
            repo,
            &pushrebase,
            bookmarks,
            hook_manager,
            streaming_clone,
            lfs,
            name,
            read_write_fetcher,
            infinitepush,
            list_keys_patterns_max,
            lca_hint,
            phases_hint,
            Arc::new(mutable_counters),
        );

        Ok(repo)
    }

    pub fn blobrepo(&self) -> &BlobRepo {
        &self.repo
    }
}
