/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use blobrepo::BlobRepo;
use blobrepo_factory::{BlobrepoBuilder, BlobstoreOptions, Caching, ReadOnlyStorage};
use context::CoreContext;
use futures::{compat::Future01CompatExt, future, FutureExt};
use hooks::HookManager;
use metaconfig_types::RepoConfig;
use mutable_counters::SqlMutableCounters;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_read_write_status::{RepoReadWriteFetcher, SqlRepoReadWriteStatus};
use skiplist::fetch_skiplist_index;
use sql_construct::{facebook::FbSqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::facebook::MysqlOptions;
use std::sync::Arc;

use crate::{streaming_clone, MononokeRepo};

pub struct MononokeRepoBuilder {
    ctx: CoreContext,
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
        let builder = BlobrepoBuilder::new(
            ctx.fb,
            name,
            &config,
            mysql_options,
            caching,
            scuba_censored_table.clone(),
            readonly_storage,
            blobstore_options.clone(),
            ctx.logger(),
        );
        let repo = builder.build().await?;

        Ok(Self {
            ctx,
            repo,
            config,
            mysql_options,
            readonly_storage,
        })
    }

    pub async fn finalize(self, hook_manager: Arc<HookManager>) -> Result<MononokeRepo, Error> {
        let Self {
            ctx,
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
            hgsql_name,
            ..
        } = config;

        let streaming_clone = async {
            if let Some(db_address) = storage_config.metadata.primary_address() {
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
                .await?;
                Ok(Some(r))
            } else {
                Ok(None)
            }
        };

        let mutable_counters = SqlMutableCounters::with_metadata_database_config(
            ctx.fb,
            &storage_config.metadata,
            mysql_options,
            readonly_storage.0,
        );

        let skiplist = fetch_skiplist_index(
            ctx.clone(),
            skiplist_index_blobstore_key,
            repo.get_blobstore().boxed(),
        )
        .compat()
        .map(|res| res.with_context(|| format!("while fetching skiplist for {}", repo.name())));

        let (streaming_clone, sql_read_write_status, mutable_counters, skiplist) =
            future::try_join4(
                streaming_clone,
                sql_read_write_status,
                mutable_counters,
                skiplist,
            )
            .await?;

        let read_write_fetcher =
            RepoReadWriteFetcher::new(sql_read_write_status, readonly, hgsql_name);

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = skiplist;

        let repo = MononokeRepo::new(
            ctx.fb,
            ctx.logger().clone(),
            repo,
            &pushrebase,
            bookmarks,
            hook_manager,
            streaming_clone,
            lfs,
            read_write_fetcher,
            infinitepush,
            list_keys_patterns_max,
            lca_hint,
            Arc::new(mutable_counters),
        );

        repo.await
    }

    pub fn blobrepo(&self) -> &BlobRepo {
        &self.repo
    }
}
