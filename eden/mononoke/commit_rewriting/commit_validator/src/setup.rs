/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use blobstore_factory::MetadataSqlFactory;
use blobstore_factory::ReadOnlyStorage;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogId;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::Large;
use cross_repo_sync::Small;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use pushredirect::SqlPushRedirectionConfigBuilder;
use repo_identity::RepoIdentityRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use sql_query_config::SqlQueryConfigArc;
use synced_commit_mapping::SqlSyncedCommitMappingBuilder;

use crate::cli::MononokeCommitValidatorArgs;
use crate::reporting::add_common_commit_syncing_fields;
use crate::validation::ValidationHelpers;
use crate::Repo;

pub async fn get_validation_helpers<'a>(
    fb: FacebookInit,
    _ctx: CoreContext,
    app: &MononokeApp,
    large_repo: Repo,
    repo_config: RepoConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    scuba_sample: MononokeScubaSampleBuilder,
) -> Result<ValidationHelpers, Error> {
    let repo_id = large_repo.repo_identity().id();

    let config_store = app.config_store();
    let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
        fb,
        repo_config.storage_config.metadata.clone(),
        mysql_options.clone(),
        readonly_storage,
    )
    .await?;
    let builder = sql_factory
        .open::<SqlPushRedirectionConfigBuilder>()
        .await?;
    let push_redirection_config = builder.build(large_repo.sql_query_config_arc());
    let live_commit_sync_config =
        CfgrLiveCommitSyncConfig::new(config_store, Arc::new(push_redirection_config))?;
    let common_commit_sync_config = live_commit_sync_config.get_common_config(repo_id)?;

    let mapping = SqlSyncedCommitMappingBuilder::with_metadata_database_config(
        fb,
        &repo_config.storage_config.metadata,
        &mysql_options,
        readonly_storage.0,
    )
    .await?
    .build(app.environment().rendezvous_options);

    let large_repo_master_bookmark =
        BookmarkKey::new(app.args::<MononokeCommitValidatorArgs>()?.master_bookmark)?;

    let validation_helper_futs =
        common_commit_sync_config
            .small_repos
            .into_keys()
            .map(|small_repo_id| {
                borrowed!(app, scuba_sample);
                cloned!(large_repo);
                async move {
                    let scuba_sample = {
                        let mut scuba_sample = scuba_sample.clone();
                        add_common_commit_syncing_fields(
                            &mut scuba_sample,
                            Large(large_repo.repo_identity().id()),
                            Small(small_repo_id),
                        );

                        scuba_sample
                    };
                    let small_repo = app.open_repo(&RepoArg::Id(small_repo_id)).await?;
                    Result::<_, Error>::Ok((
                        small_repo_id,
                        (Large(large_repo), Small(small_repo), scuba_sample),
                    ))
                }
            });

    let validation_helpers = try_join_all(validation_helper_futs).await?;

    Ok(ValidationHelpers::new(
        large_repo,
        validation_helpers.into_iter().collect(),
        large_repo_master_bookmark,
        mapping,
        live_commit_sync_config,
    ))
}

pub fn format_counter() -> String {
    "x_repo_commit_validator".to_string()
}

pub async fn get_start_id<'a>(
    ctx: &CoreContext,
    repo: &impl MutableCountersRef,
    start_id: Option<BookmarkUpdateLogId>,
) -> Result<BookmarkUpdateLogId, Error> {
    match start_id {
        Some(start_id) => Ok(start_id),
        None => {
            let counter = format_counter();
            repo.mutable_counters()
                .get_counter(ctx, &counter)
                .await?
                .map(|val| val.try_into())
                .transpose()?
                .ok_or_else(|| format_err!("mutable counter {} is missing", counter))
        }
    }
}
