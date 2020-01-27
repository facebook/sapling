/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{format_err, Error};
use cloned::cloned;
use failure_ext::chain::ChainExt;
use futures::{future, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
};
use slog::{info, o, Logger};

use backsyncer::open_backsyncer_dbs_compat;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use blobstore_factory::make_blobstore_no_sql;
use cache_warmup::cache_warmup;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use fbinit::FacebookInit;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{blobrepo_text_only_store, BlobRepoChangesetStore};
use metaconfig_types::{CommitSyncConfig, MetadataDBConfig, RepoConfig, WireprotoLoggingConfig};
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use ready_state::ReadyStateBuilder;
use repo_client::{MononokeRepo, MononokeRepoBuilder, PushRedirector, WireprotoLogging};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use sql_ext::MysqlOptions;
use sql_ext::SqlConstructors;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::errors::ErrorKind;

/// An auxillary struct to pass between closures before we
/// are capable of creating a full `RepoHandler`
/// To create `RepoHandler`, we need to look at various
/// fields of such struct for other repos, so we first
/// have to construct all `IncompleteRepoHandler`s and
/// only then can we populate the `PushRedirector`
#[derive(Clone)]
struct IncompleteRepoHandler {
    logger: Logger,
    scuba: ScubaSampleBuilder,
    wireproto_logging: Arc<WireprotoLogging>,
    repo: MononokeRepo,
    hash_validation_percentage: usize,
    preserve_raw_bundle2: bool,
    pure_push_allowed: bool,
    support_bundle2_listkeys: bool,
}

impl IncompleteRepoHandler {
    fn into_repo_handler_with_push_redirector(
        self,
        maybe_push_redirector: Option<PushRedirector>,
    ) -> RepoHandler {
        let IncompleteRepoHandler {
            logger,
            scuba,
            wireproto_logging,
            repo,
            hash_validation_percentage,
            preserve_raw_bundle2,
            pure_push_allowed,
            support_bundle2_listkeys,
        } = self;
        RepoHandler {
            logger,
            scuba,
            wireproto_logging,
            repo,
            hash_validation_percentage,
            preserve_raw_bundle2,
            pure_push_allowed,
            support_bundle2_listkeys,
            maybe_push_redirector,
        }
    }
}

/// An auxillary struct to pass between closures before
/// we are capable of creating a full `PushRedirector`
#[derive(Clone)]
struct PushRedirectorArgs {
    commit_sync_config: CommitSyncConfig,
    synced_commit_mapping: SqlSyncedCommitMapping,
    db_config: MetadataDBConfig,
    mysql_options: MysqlOptions,
}

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_logging: Arc<WireprotoLogging>,
    pub repo: MononokeRepo,
    pub hash_validation_percentage: usize,
    pub preserve_raw_bundle2: bool,
    pub pure_push_allowed: bool,
    pub support_bundle2_listkeys: bool,
    pub maybe_push_redirector: Option<PushRedirector>,
}

/// Given a `CommitSyncConfig`, a small repo id, and an
/// auxillary struct, that holds partially built `RepoHandler`,
/// build `PushRedirector` for a push rediction from this
/// small repo into a large repo.
fn create_push_redirector(
    ctx: CoreContext,
    source_repo: &MononokeRepo,
    target_incomplete_repo_handler: &IncompleteRepoHandler,
    push_redirector_args: PushRedirectorArgs,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<PushRedirector, Error> {
    let PushRedirectorArgs {
        commit_sync_config,
        synced_commit_mapping,
        db_config,
        mysql_options,
    } = push_redirector_args;

    let small_repo = source_repo.blobrepo().clone();
    let large_repo = target_incomplete_repo_handler.repo.blobrepo().clone();
    let mapping: Arc<dyn SyncedCommitMapping> = Arc::new(synced_commit_mapping);
    let syncers = try_boxfuture!(create_commit_syncers(
        small_repo.clone(),
        large_repo,
        &commit_sync_config,
        mapping.clone()
    ));

    let small_to_large_commit_syncer = syncers.small_to_large;
    let large_to_small_commit_syncer = syncers.large_to_small;

    let repo = target_incomplete_repo_handler.repo.clone();

    open_backsyncer_dbs_compat(
        ctx.clone(),
        small_repo,
        db_config,
        mysql_options,
        readonly_storage,
    )
    .map(move |target_repo_dbs| PushRedirector {
        repo,
        small_to_large_commit_syncer,
        large_to_small_commit_syncer,
        target_repo_dbs,
        commit_sync_config,
    })
    .boxify()
}

fn get_maybe_create_push_redirector_fut(
    ctx: CoreContext,
    incomplete_repo_handler: &IncompleteRepoHandler,
    push_redirector_args: PushRedirectorArgs,
    lookup_table: &HashMap<RepositoryId, IncompleteRepoHandler>,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<Option<PushRedirector>, Error> {
    let large_repo_id = push_redirector_args.commit_sync_config.large_repo_id;
    let current_repo_id = incomplete_repo_handler.repo.repoid();
    let current_repo = &incomplete_repo_handler.repo;
    let target_incomplete_repo_handler = try_boxfuture!(lookup_table
        .get(&large_repo_id)
        .ok_or(ErrorKind::LargeRepoNotFound(large_repo_id)));

    if large_repo_id == current_repo_id {
        future::ok(None).boxify()
    } else {
        create_push_redirector(
            ctx,
            current_repo,
            target_incomplete_repo_handler,
            push_redirector_args,
            readonly_storage,
        )
        .map(Some)
        .boxify()
    }
}

pub fn repo_handlers(
    fb: FacebookInit,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    mysql_options: MysqlOptions,
    caching: Caching,
    mut disabled_hooks: HashMap<String, HashSet<String>>,
    scuba_censored_table: Option<String>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    root_log: &Logger,
    ready: &mut ReadyStateBuilder,
) -> BoxFuture<HashMap<String, RepoHandler>, Error> {
    // compute eagerly to avoid lifetime issues
    let repo_futs: Vec<
        BoxFuture<
            (
                CoreContext,
                String,
                IncompleteRepoHandler,
                Option<PushRedirectorArgs>,
            ),
            Error,
        >,
    > = repos
        .into_iter()
        .filter(|(reponame, config)| {
            if !config.enabled {
                info!(root_log, "Repo {} not enabled", reponame)
            };
            config.enabled
        })
        .map(|(reponame, config)| {
            let root_log = root_log.clone();
            let logger = root_log.new(o!("repo" => reponame.clone()));
            let ctx = CoreContext::new_with_logger(fb, logger.clone());

            let ready_handle = ready.create_handle(reponame.clone());

            let disabled_hooks = disabled_hooks.remove(&reponame).unwrap_or(HashSet::new());

            // Clone the few things we're going to need later in our bootstrap.
            let cache_warmup_params = config.cache_warmup.clone();
            let scuba_table = config.scuba_table.clone();
            let hooks_scuba_table = config.scuba_table_hooks.clone();
            let hook_max_file_size = config.hook_max_file_size.clone();
            let db_config = config.storage_config.dbconfig.clone();
            let hash_validation_percentage = config.hash_validation_percentage.clone();
            let preserve_raw_bundle2 = config.bundle2_replay_params.preserve_raw_bundle2.clone();
            let pure_push_allowed = config.push.pure_push_allowed.clone();
            let wireproto_logging = config.wireproto_logging.clone();
            let commit_sync_config = config.commit_sync_config.clone();
            let hook_manager_params = config.hook_manager_params.clone();

            // TODO: Don't require full config in load_hooks so we can avoid cloning the entire
            // config here.
            let hook_config = config.clone();

            // And clone a few things of which we only have one but which we're going to need one
            // per repo.
            let blobstore_options = blobstore_options.clone();
            let scuba_censored_table = scuba_censored_table.clone();

            let fut = async move {
                info!(logger, "Opening blobrepo");
                let builder = MononokeRepoBuilder::prepare(
                    ctx.clone(),
                    reponame.clone(),
                    config,
                    mysql_options,
                    caching,
                    scuba_censored_table.clone(),
                    readonly_storage,
                    blobstore_options,
                )
                .await?;

                let blobrepo = builder.blobrepo().clone();

                info!(logger, "Warming up cache");
                let initial_warmup = tokio_preview::task::spawn(
                    cache_warmup(ctx.clone(), blobrepo.clone(), cache_warmup_params)
                        .chain_err(format!("while warming up cache for repo: {}", reponame))
                        .map_err(Error::from)
                        .compat()
                        .boxed(),
                );

                let mut scuba_logger = ScubaSampleBuilder::with_opt_table(fb, scuba_table);
                scuba_logger.add_common_server_data();

                let mut hooks_scuba = ScubaSampleBuilder::with_opt_table(fb, hooks_scuba_table);
                hooks_scuba.add("repo", reponame.clone());

                info!(logger, "Creating HookManager");
                let mut hook_manager = HookManager::new(
                    ctx.fb,
                    Box::new(BlobRepoChangesetStore::new(blobrepo.clone())),
                    blobrepo_text_only_store(blobrepo.clone(), hook_max_file_size),
                    hook_manager_params.unwrap_or(Default::default()),
                    hooks_scuba,
                );

                info!(logger, "Loading hooks");
                load_hooks(fb, &mut hook_manager, hook_config, &disabled_hooks)?;

                let repo = builder.finalize(Arc::new(hook_manager));

                let support_bundle2_listkeys = async {
                    let counters = SqlMutableCounters::with_db_config(
                        fb,
                        &db_config,
                        mysql_options,
                        readonly_storage.0,
                    )
                    .compat()
                    .await?;

                    let counter = counters
                        .get_counter(
                            ctx.clone(),
                            blobrepo.get_repoid(),
                            "support_bundle2_listkeys",
                        )
                        .compat()
                        .await?
                        .unwrap_or(1);
                    Ok(counter != 0)
                };

                let sql_commit_sync_mapping = SqlSyncedCommitMapping::with_db_config(
                    fb,
                    &db_config,
                    mysql_options,
                    readonly_storage.0,
                )
                .compat();

                let wireproto_logging = create_wireproto_logging(
                    fb,
                    reponame.clone(),
                    readonly_storage,
                    wireproto_logging,
                )
                .compat();

                let (repo, support_bundle2_listkeys, sql_commit_sync_mapping, wireproto_logging) =
                    futures_preview::future::try_join4(
                        repo,
                        support_bundle2_listkeys,
                        sql_commit_sync_mapping,
                        wireproto_logging,
                    )
                    .await?;

                let maybe_push_redirector_args =
                    commit_sync_config.map(move |commit_sync_config| PushRedirectorArgs {
                        commit_sync_config,
                        synced_commit_mapping: sql_commit_sync_mapping,
                        db_config,
                        mysql_options,
                    });

                initial_warmup.await??;

                info!(logger, "Repository is ready");
                Ok((
                    ctx,
                    reponame,
                    IncompleteRepoHandler {
                        logger,
                        scuba: scuba_logger,
                        wireproto_logging: Arc::new(wireproto_logging),
                        repo,
                        hash_validation_percentage,
                        preserve_raw_bundle2,
                        pure_push_allowed,
                        support_bundle2_listkeys,
                    },
                    maybe_push_redirector_args,
                ))
            };

            ready_handle.wait_for(fut.boxed().compat()).boxify()
        })
        .collect();

    future::join_all(repo_futs)
        .and_then(move |t| build_repo_handlers(t, readonly_storage))
        .boxify()
}

fn build_repo_handlers(
    tuples: Vec<(
        CoreContext,
        String,
        IncompleteRepoHandler,
        Option<PushRedirectorArgs>,
    )>,
    readonly_storage: ReadOnlyStorage,
) -> impl Future<Item = HashMap<String, RepoHandler>, Error = Error> {
    let lookup_table: HashMap<RepositoryId, IncompleteRepoHandler> = tuples
        .iter()
        .map(|(_, _, incomplete_repo_handler, _)| {
            (
                incomplete_repo_handler.repo.repoid(),
                incomplete_repo_handler.clone(),
            )
        })
        .collect();

    future::join_all({
        cloned!(lookup_table);
        tuples.into_iter().map(
            move |(ctx, reponame, incomplete_repo_handler, maybe_push_redirector_args)| {
                let maybe_push_redirector_fut = match maybe_push_redirector_args {
                    None => future::ok(None).boxify(),
                    Some(push_redirector_args) => get_maybe_create_push_redirector_fut(
                        ctx.clone(),
                        &incomplete_repo_handler,
                        push_redirector_args,
                        &lookup_table,
                        readonly_storage,
                    ),
                };

                maybe_push_redirector_fut
                    .map(move |maybe_push_redirector| {
                        (
                            reponame,
                            incomplete_repo_handler
                                .into_repo_handler_with_push_redirector(maybe_push_redirector),
                        )
                    })
                    .boxify()
            },
        )
    })
    .map(|v| v.into_iter().collect())
}

fn create_wireproto_logging(
    fb: FacebookInit,
    reponame: String,
    readonly_storage: ReadOnlyStorage,
    wireproto_logging_config: WireprotoLoggingConfig,
) -> impl Future<Item = WireprotoLogging, Error = Error> {
    let WireprotoLoggingConfig {
        storage_config_and_threshold,
        scribe_category,
    } = wireproto_logging_config;
    let blobstore_fut = match storage_config_and_threshold {
        Some((storage_config, threshold)) => {
            if readonly_storage.0 {
                return future::err(format_err!(
                    "failed to create blobstore for wireproto logging because storage is readonly",
                ))
                .right_future();
            }
            make_blobstore_no_sql(fb, &storage_config.blobstore, readonly_storage)
                .map(move |blobstore| Some((blobstore, threshold)))
                .left_future()
        }
        None => future::ok(None).right_future(),
    };

    blobstore_fut
        .map(move |blobstore_and_threshold| {
            WireprotoLogging::new(fb, reponame, scribe_category, blobstore_and_threshold)
        })
        .left_future()
}
