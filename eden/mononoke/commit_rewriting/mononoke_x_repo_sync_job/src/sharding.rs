/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore_factory::MetadataSqlFactory;
use bookmarks::BookmarkKey;
use cmdlib::helpers;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use context::CoreContext;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::PushrebaseRewriteDates;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use futures::FutureExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::RepoConfigRef;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::DerivableType;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersArc;
use pushredirect::SqlPushRedirectionConfigBuilder;
use regex::Regex;
use repo_identity::RepoIdentityRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sharding_ext::encode_repo_name;
use sharding_ext::RepoShard;
use slog::info;
use sql_query_config::SqlQueryConfigArc;
use zk_leader_election::LeaderElection;
use zk_leader_election::ZkMode;

use crate::cli::ForwardSyncerCommand;
use crate::reporting::add_common_fields;
use crate::run_in_initial_import_mode;
use crate::run_in_single_commit_mode;
use crate::run_in_tailing_mode;
use crate::sharding::ForwardSyncerCommand::InitialImport;
use crate::sharding::ForwardSyncerCommand::Once;
use crate::sharding::ForwardSyncerCommand::Tail;
use crate::BackpressureParams;
use crate::ForwardSyncerArgs;
use crate::Repo;
use crate::TailingArgs;

const JOB_NAME: &str = "mononoke_x_repo_sync_job";

/// Struct representing the X Repo Sync Sharded Process
pub struct XRepoSyncProcess {
    ctx: Arc<CoreContext>,
    pub(crate) app: Arc<MononokeApp>,
    pub(crate) args: Arc<ForwardSyncerArgs>,
}

impl XRepoSyncProcess {
    pub(crate) fn new(
        ctx: Arc<CoreContext>,
        app: Arc<MononokeApp>,
        args: Arc<ForwardSyncerArgs>,
    ) -> Self {
        Self { ctx, app, args }
    }
}

#[async_trait]
impl RepoShardedProcess for XRepoSyncProcess {
    async fn setup(&self, repo: &RepoShard) -> Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.ctx.logger();
        let small_repo_name = repo.repo_name.to_string();
        let large_repo_name = repo
            .target_repo_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No large repo name provided for forward syncer"))?;
        info!(
            &logger,
            "Adding small repo {small_repo_name} and large repo {large_repo_name} to X Repo Sync"
        );
        let repo_args = SourceAndTargetRepoArgs::with_source_and_target_repo_name(
            small_repo_name,
            large_repo_name.to_string(),
        );
        let x_repo_sync_process_executor = XRepoSyncProcessExecutor::new(
            self.app.clone(),
            self.ctx.clone(),
            self.args.clone(),
            &repo_args,
        )
        .await?;
        Ok(Arc::new(x_repo_sync_process_executor))
    }
}

/// Struct representing the X Repo Sync Sharded Process Executor
pub struct XRepoSyncProcessExecutor {
    app: Arc<MononokeApp>,
    ctx: Arc<CoreContext>,
    scuba_sample: MononokeScubaSampleBuilder,
    args: Arc<ForwardSyncerArgs>,
    small_repo: Arc<Repo>,
    large_repo: Arc<Repo>,
    common_bookmarks: HashSet<BookmarkKey>,
    target_mutable_counters: Arc<dyn MutableCounters + Send + Sync>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    commit_syncer: CommitSyncer<Arc<Repo>>,
    live_commit_sync_config: Arc<CfgrLiveCommitSyncConfig>,
}

impl XRepoSyncProcessExecutor {
    pub(crate) async fn new(
        app: Arc<MononokeApp>,
        ctx: Arc<CoreContext>,
        args: Arc<ForwardSyncerArgs>,
        repo_args: &SourceAndTargetRepoArgs,
    ) -> Result<Self> {
        let small_repo: Arc<Repo> = app.open_repo_unredacted(&repo_args.source_repo).await?;
        let large_repo: Arc<Repo> = app.open_repo_unredacted(&repo_args.target_repo).await?;
        let syncers =
            create_commit_syncers_from_app(&ctx, &app, small_repo.clone(), large_repo.clone())
                .await?;
        let config_store = app.environment().config_store.clone();
        let commit_syncer = syncers.small_to_large;
        let mut scuba_sample = ctx.scuba().clone();
        let (_, repo_config) = app.repo_config(repo_args.source_repo.as_repo_arg())?;
        let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
            app.fb,
            repo_config.storage_config.metadata.clone(),
            app.mysql_options().clone(),
            *app.readonly_storage(),
        )
        .await?;
        let builder = sql_factory
            .open::<SqlPushRedirectionConfigBuilder>()
            .await?;
        let push_redirection_config = builder.build(small_repo.sql_query_config_arc());
        let live_commit_sync_config = Arc::new(CfgrLiveCommitSyncConfig::new(
            &config_store,
            Arc::new(push_redirection_config),
        )?);
        let common_commit_sync_config =
            live_commit_sync_config.get_common_config(small_repo.repo_identity().id())?;

        let common_bookmarks: HashSet<_> = common_commit_sync_config
            .common_pushrebase_bookmarks
            .clone()
            .into_iter()
            .collect();

        let target_mutable_counters = large_repo.mutable_counters_arc();

        let pushrebase_rewrite_dates = if args.pushrebase_rewrite_dates {
            PushrebaseRewriteDates::Yes
        } else {
            PushrebaseRewriteDates::No
        };

        add_common_fields(&mut scuba_sample, &commit_syncer);
        Ok(Self {
            app,
            ctx,
            scuba_sample,
            args,
            small_repo,
            large_repo,
            common_bookmarks,
            target_mutable_counters,
            pushrebase_rewrite_dates,
            commit_syncer,
            live_commit_sync_config,
        })
    }

    async fn process_command(&self) -> Result<()> {
        let ctx = &self.ctx;
        match &self.args.command {
            InitialImport(initial_import_args) => {
                let sync_config_version_name = initial_import_args.sync_config_version_name.clone();
                let config_version = CommitSyncConfigVersion(sync_config_version_name);
                let resolved_csids = initial_import_args
                    .changeset_args
                    .resolve_changesets(ctx, &self.small_repo)
                    .boxed()
                    .await?;

                run_in_initial_import_mode(
                    ctx,
                    resolved_csids,
                    self.commit_syncer.clone(),
                    config_version,
                    self.scuba_sample.clone(),
                    initial_import_args.no_progress_bar,
                    initial_import_args.no_automatic_derivation,
                    initial_import_args.derivation_batch_size,
                )
                .await
            }
            Once(once_cmd_args) => {
                let maybe_target_bookmark = once_cmd_args
                    .target_bookmark
                    .clone()
                    .map(BookmarkKey::new)
                    .transpose()?;
                let bcs =
                    helpers::csid_resolve(ctx, &self.small_repo, &once_cmd_args.commit.as_str())
                        .await?;
                let new_version = once_cmd_args
                    .new_version
                    .clone()
                    .map(CommitSyncConfigVersion);

                run_in_single_commit_mode(
                    ctx,
                    bcs,
                    self.commit_syncer.clone(),
                    self.scuba_sample.clone(),
                    maybe_target_bookmark,
                    self.common_bookmarks.clone(),
                    self.pushrebase_rewrite_dates,
                    new_version,
                    once_cmd_args.unsafe_force_rewrite_parent_to_target_bookmark,
                )
                .await
            }
            Tail(tail_cmd_args) => {
                let sleep_duration = Duration::from_secs(tail_cmd_args.sleep_secs);
                let tailing_args = if tail_cmd_args.catch_up_once {
                    TailingArgs::CatchUpOnce(self.commit_syncer.clone())
                } else {
                    TailingArgs::LoopForever(self.commit_syncer.clone())
                };

                let maybe_bookmark_regex =
                    self.bookmark_regex(tail_cmd_args.bookmark_regex.as_ref())?;

                let backpressure_params =
                    BackpressureParams::new(&self.app, tail_cmd_args.clone()).await?;

                run_in_tailing_mode(
                    ctx,
                    self.target_mutable_counters.clone(),
                    self.common_bookmarks.clone(),
                    self.scuba_sample.clone(),
                    backpressure_params,
                    tail_cmd_args
                        .derived_data_types
                        .clone()
                        .into_iter()
                        .map(|ty| DerivableType::from_name(&ty))
                        .collect::<Result<_>>()?,
                    tailing_args,
                    sleep_duration,
                    maybe_bookmark_regex,
                    self.pushrebase_rewrite_dates.clone(),
                    self.live_commit_sync_config.clone(),
                )
                .boxed()
                .await
            }
        }
    }

    fn bookmark_regex(&self, cli_bookmark_regex: Option<&String>) -> Result<Option<Regex>> {
        // The CLI arguments override the config provided value
        if let Some(regex) = cli_bookmark_regex {
            let regex = Regex::new(regex.as_str())?;
            Ok(Some(regex))
        } else if let Some(configs) = self
            .small_repo
            .repo_config()
            .x_repo_sync_source_mapping
            .as_ref()
        {
            let large_repo_name = self.large_repo.repo_identity().name();
            let regex = configs
                .mapping
                .get(large_repo_name)
                .map(|config| Regex::new(config.bookmark_regex.as_str()))
                .transpose()?;
            Ok(regex)
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for XRepoSyncProcessExecutor {
    async fn execute(&self) -> Result<()> {
        let small_repo_name = self.small_repo.repo_identity().name();
        let large_repo_name = self.large_repo.repo_identity().name();
        info!(
            self.ctx.logger(),
            "Starting up X Repo Sync from small repo {small_repo_name} to large repo {large_repo_name}"
        );
        let mode: ZkMode = self.args.leader_only.into();
        let guard = self.maybe_become_leader(mode, self.ctx.logger().clone())
            .await.with_context(|| format!("Failed to become leader for X Repo Sync from small repo {small_repo_name} to large repo {large_repo_name}"))?;
        if guard.is_some() {
            let use_sharded_job = justknobs::eval(
                "scm/mononoke:use_sharded_x_repo_sync_job",
                None,
                Some(small_repo_name),
            )?;
            if !use_sharded_job {
                info!(
                    self.ctx.logger(),
                    "Skipping X Repo Sync from small repo {small_repo_name} to large repo {large_repo_name} because of JK"
                );
                return Ok(());
            } else {
                info!(
                    self.ctx.logger(),
                    "Became leader for X Repo Sync from small repo {small_repo_name} to large repo {large_repo_name}"
                );
            }
        }
        let result = self.process_command()
        .await
        .with_context(|| {
            format!(
                "Error encountered during X Repo Sync execution from small repo {small_repo_name} to large repo {large_repo_name}"
            )
        });
        if let Err(e) = &result {
            let mut scuba = self.ctx.scuba().clone();
            scuba.add("error", e.to_string()).add("status", "failure");
            scuba.log();
        }
        result?;
        info!(
            self.ctx.logger(),
            "X Repo Sync execution finished from small repo {small_repo_name} to large repo {large_repo_name}"
        );
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let small_repo_name = self.small_repo.repo_identity().name();
        let large_repo_name = self.large_repo.repo_identity().name();
        info!(
            self.ctx.logger(),
            "Shutting down X Repo Sync from small repo {small_repo_name} to large repo {large_repo_name}"
        );
        Ok(())
    }
}

#[async_trait]
impl LeaderElection for XRepoSyncProcessExecutor {
    fn get_shared_lock_path(&self) -> String {
        let small_repo_name = encode_repo_name(self.small_repo.repo_identity().name());
        let large_repo_name = encode_repo_name(self.large_repo.repo_identity().name());
        format!("{JOB_NAME}_{small_repo_name}_{large_repo_name}")
    }
}
