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
use futures::{future, Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::{info, o, Logger};

use backsyncer::open_backsyncer_dbs_compat;
use blobrepo_factory::{open_blobrepo, BlobstoreOptions, Caching, ReadOnlyStorage};
use blobstore_factory::make_blobstore_no_sql;
use cache_warmup::cache_warmup;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use fbinit::FacebookInit;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{blobrepo_text_only_store, BlobRepoChangesetStore};
use metaconfig_types::{
    CommitSyncConfig, MetadataDBConfig, RepoConfig, StorageConfig, WireprotoLoggingConfig,
};
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use phases::{CachingPhases, Phases, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use ready_state::ReadyStateBuilder;
use repo_client::{
    streaming_clone, MononokeRepo, PushRedirector, RepoReadWriteFetcher, WireprotoLogging,
};
use repo_read_write_status::SqlRepoReadWriteStatus;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use skiplist::fetch_skiplist_index;
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

fn open_db_from_config<S: SqlConstructors>(
    fb: FacebookInit,
    dbconfig: &MetadataDBConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<S, Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { ref path } => {
            S::with_sqlite_path(path.join("sqlite_dbs"), readonly_storage.0)
                .into_future()
                .boxify()
        }
        MetadataDBConfig::Mysql { ref db_address, .. } => {
            S::with_xdb(fb, db_address.clone(), mysql_options, readonly_storage.0)
        }
    }
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
    disabled_hooks: &HashMap<String, HashSet<String>>,
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
            info!(logger, "Opening blobrepo");

            let ready_handle = ready.create_handle(reponame.clone());

            let repoid = config.repoid;
            let disabled_hooks = disabled_hooks
                .get(&reponame)
                .cloned()
                .unwrap_or(HashSet::new());

            open_blobrepo(
                fb,
                config.storage_config.clone(),
                repoid,
                mysql_options,
                caching,
                config.bookmarks_cache_ttl,
                config.redaction,
                scuba_censored_table.clone(),
                config.filestore.clone(),
                readonly_storage,
                blobstore_options.clone(),
                logger.clone(),
            )
            .and_then(move |blobrepo| {
                let RepoConfig {
                    storage_config: StorageConfig { dbconfig, .. },
                    cache_warmup: cache_warmup_params,
                    hook_manager_params,
                    write_lock_db_address,
                    readonly,
                    pushrebase,
                    bookmarks,
                    lfs,
                    infinitepush,
                    list_keys_patterns_max,
                    scuba_table,
                    scuba_table_hooks,
                    hash_validation_percentage,
                    wireproto_logging,
                    bundle2_replay_params,
                    push,
                    hook_max_file_size,
                    commit_sync_config,
                    ..
                } = config.clone();

                let hook_manager_params = hook_manager_params.unwrap_or(Default::default());

                let mut scuba = if let Some(table_name) = scuba_table_hooks {
                    ScubaSampleBuilder::new(fb, table_name)
                } else {
                    ScubaSampleBuilder::with_discard()
                };
                scuba.add("repo", reponame.clone());

                info!(logger, "Creating HookManager");
                let mut hook_manager = HookManager::new(
                    ctx.clone(),
                    Box::new(BlobRepoChangesetStore::new(blobrepo.clone())),
                    blobrepo_text_only_store(blobrepo.clone(), hook_max_file_size),
                    hook_manager_params,
                    scuba,
                );

                // TODO: Don't require full config in load_hooks so we can avoid a clone here.
                info!(logger, "Loading hooks");
                try_boxfuture!(load_hooks(
                    fb,
                    &mut hook_manager,
                    config.clone(),
                    &disabled_hooks
                ));

                let streaming_clone = if let Some(db_address) = dbconfig.get_db_address() {
                    streaming_clone(
                        fb,
                        blobrepo.clone(),
                        db_address,
                        mysql_options,
                        repoid,
                        readonly_storage.0,
                    )
                    .map(Some)
                    .left_future()
                } else {
                    Ok(None).into_future().right_future()
                };

                // XXX Fixme - put write_lock_db_address into storage_config.dbconfig?
                let sql_read_write_status = if let Some(addr) = write_lock_db_address {
                    SqlRepoReadWriteStatus::with_xdb(fb, addr, mysql_options, readonly_storage.0)
                        .map(Some)
                        .left_future()
                } else {
                    Ok(None).into_future().right_future()
                };

                let sql_mutable_counters = open_db_from_config::<SqlMutableCounters>(
                    fb,
                    &dbconfig,
                    mysql_options,
                    readonly_storage,
                );

                let phases_hint = open_db_from_config::<SqlPhases>(
                    fb,
                    &dbconfig,
                    mysql_options,
                    readonly_storage,
                );

                let sql_commit_sync_mapping = open_db_from_config::<SqlSyncedCommitMapping>(
                    fb,
                    &dbconfig,
                    mysql_options,
                    readonly_storage,
                );

                streaming_clone
                    .join5(
                        sql_read_write_status,
                        sql_mutable_counters,
                        phases_hint,
                        sql_commit_sync_mapping,
                    )
                    .and_then(
                        move |(
                            streaming_clone,
                            sql_read_write_status,
                            sql_mutable_counters,
                            phases_hint,
                            sql_commit_sync_mapping,
                        )| {
                            let read_write_fetcher = RepoReadWriteFetcher::new(
                                sql_read_write_status,
                                readonly,
                                reponame.clone(),
                            );

                            let listen_log = root_log.new(o!("repo" => reponame.clone()));
                            let mut scuba_logger =
                                ScubaSampleBuilder::with_opt_table(fb, scuba_table);
                            scuba_logger.add_common_server_data();
                            let hash_validation_percentage = hash_validation_percentage;
                            let preserve_raw_bundle2 = bundle2_replay_params.preserve_raw_bundle2;
                            let pure_push_allowed = push.pure_push_allowed;

                            let skip_index = fetch_skiplist_index(
                                ctx.clone(),
                                config.skiplist_index_blobstore_key,
                                blobrepo.get_blobstore().boxed(),
                            );

                            info!(logger, "Warming up cache");
                            let initial_warmup =
                                cache_warmup(ctx.clone(), blobrepo.clone(), cache_warmup_params)
                                    .chain_err(format!(
                                        "while warming up cache for repo: {}",
                                        reponame
                                    ))
                                    .from_err();

                            let mutable_counters = Arc::new(sql_mutable_counters);

                            // TODO: T45466266 this should be replaced by gatekeepers
                            let support_bundle2_listkeys = mutable_counters
                                .clone()
                                .get_counter(
                                    ctx.clone(),
                                    blobrepo.get_repoid(),
                                    "support_bundle2_listkeys",
                                )
                                .map(|val| val.unwrap_or(1) != 0);

                            let wireproto_logging_fut = create_wireproto_logging(
                                fb,
                                reponame.clone(),
                                readonly_storage,
                                wireproto_logging,
                            )
                            .map(Arc::new);

                            ready_handle
                                .wait_for(initial_warmup.and_then(|()| {
                                    skip_index
                                        .join3(support_bundle2_listkeys, wireproto_logging_fut)
                                }))
                                .map({
                                    move |(
                                        skip_index,
                                        support_bundle2_listkeys,
                                        wireproto_logging,
                                    )| {
                                        // initialize phases hint from the skip index
                                        let phases_hint: Arc<dyn Phases> =
                                            if let MetadataDBConfig::Mysql { .. } = dbconfig.clone()
                                            {
                                                Arc::new(CachingPhases::new(
                                                    fb,
                                                    Arc::new(phases_hint),
                                                ))
                                            } else {
                                                Arc::new(phases_hint)
                                            };

                                        // initialize lca hint from the skip index
                                        let lca_hint: Arc<dyn LeastCommonAncestorsHint> =
                                            skip_index;

                                        let repo = MononokeRepo::new(
                                            blobrepo,
                                            &pushrebase,
                                            bookmarks.clone(),
                                            Arc::new(hook_manager),
                                            streaming_clone,
                                            lfs,
                                            reponame.clone(),
                                            read_write_fetcher,
                                            infinitepush,
                                            list_keys_patterns_max,
                                            lca_hint,
                                            phases_hint,
                                            mutable_counters,
                                        );

                                        let maybe_push_redirector_args =
                                            commit_sync_config.map(move |commit_sync_config| {
                                                PushRedirectorArgs {
                                                    commit_sync_config,
                                                    synced_commit_mapping: sql_commit_sync_mapping,
                                                    db_config: dbconfig,
                                                    mysql_options,
                                                }
                                            });

                                        info!(logger, "Repository is ready");
                                        (
                                            ctx,
                                            reponame,
                                            IncompleteRepoHandler {
                                                logger: listen_log,
                                                scuba: scuba_logger,
                                                wireproto_logging,
                                                repo,
                                                hash_validation_percentage,
                                                preserve_raw_bundle2,
                                                pure_push_allowed,
                                                support_bundle2_listkeys,
                                            },
                                            maybe_push_redirector_args,
                                        )
                                    }
                                })
                                .boxify()
                        },
                    )
                    .boxify()
            })
            .boxify()
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
