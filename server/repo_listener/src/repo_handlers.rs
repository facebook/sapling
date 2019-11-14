/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cloned::cloned;
use failure_ext::prelude::*;
use futures::{future, Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::{info, o, Logger};

use backsyncer::open_backsyncer_dbs_compat;
use blobrepo_factory::{open_blobrepo, Caching, ReadOnlyStorage};
use cache_warmup::cache_warmup;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use fbinit::FacebookInit;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{blobrepo_text_only_store, BlobRepoChangesetStore};
use metaconfig_types::{CommitSyncConfig, MetadataDBConfig, RepoConfig, StorageConfig};
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use phases::{CachingPhases, Phases, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use ready_state::ReadyStateBuilder;
use repo_client::{
    streaming_clone, MononokeRepo, RepoReadWriteFetcher, RepoSyncTarget, WireprotoLogging,
};
use repo_read_write_status::SqlRepoReadWriteStatus;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use skiplist::fetch_skiplist_index;
use sql_ext::SqlConstructors;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::errors::ErrorKind;

/// An auxillary struct to pass between closures before we
/// are capable of creating a full `RepoHandler`
/// To create `RepoHandler`, we need to look at various
/// fields of such struct for other repos, so we first
/// have to construct all `IncompleteRepoHandler`s and
/// only then can we populate the `RepoSyncTarget`
#[derive(Clone)]
struct IncompleteRepoHandler {
    logger: Logger,
    scuba: ScubaSampleBuilder,
    wireproto_logging: Option<Arc<WireprotoLogging>>,
    repo: MononokeRepo,
    hash_validation_percentage: usize,
    preserve_raw_bundle2: bool,
    pure_push_allowed: bool,
    support_bundle2_listkeys: bool,
}

impl IncompleteRepoHandler {
    fn into_repo_handler_with_sync_target(
        self,
        maybe_repo_sync_target: Option<RepoSyncTarget>,
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
            maybe_repo_sync_target,
        }
    }
}

/// An auxillary struct to pass between closures before
/// we are capable of creating a full `RepoSyncTarget`
#[derive(Clone)]
struct RepoSyncTargetArgs {
    commit_sync_config: CommitSyncConfig,
    synced_commit_mapping: SqlSyncedCommitMapping,
    db_config: MetadataDBConfig,
    maybe_myrouter_port: Option<u16>,
}

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_logging: Option<Arc<WireprotoLogging>>,
    pub repo: MononokeRepo,
    pub hash_validation_percentage: usize,
    pub preserve_raw_bundle2: bool,
    pub pure_push_allowed: bool,
    pub support_bundle2_listkeys: bool,
    pub maybe_repo_sync_target: Option<RepoSyncTarget>,
}

fn open_db_from_config<S: SqlConstructors>(
    dbconfig: &MetadataDBConfig,
    myrouter_port: Option<u16>,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<S, Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { ref path } => S::with_sqlite_path(path.join("sqlite_dbs"))
            .into_future()
            .boxify(),
        MetadataDBConfig::Mysql { ref db_address, .. } => {
            S::with_xdb(db_address.clone(), myrouter_port, readonly_storage.0)
        }
    }
}

/// Given a `CommitSyncConfig`, a small repo id, and an
/// auxillary struct, that holds partially built `RepoHandler`,
/// build `RepoSyncTarget` for a push rediction from this
/// small repo into a large repo.
fn create_repo_sync_target(
    ctx: CoreContext,
    source_repo: &MononokeRepo,
    target_incomplete_repo_handler: &IncompleteRepoHandler,
    repo_sync_target_args: RepoSyncTargetArgs,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<RepoSyncTarget, Error> {
    let RepoSyncTargetArgs {
        commit_sync_config,
        synced_commit_mapping,
        db_config,
        maybe_myrouter_port,
    } = repo_sync_target_args;

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
        maybe_myrouter_port,
        readonly_storage,
    )
    .map(move |target_repo_dbs| RepoSyncTarget {
        repo,
        small_to_large_commit_syncer,
        large_to_small_commit_syncer,
        target_repo_dbs,
        commit_sync_config,
    })
    .boxify()
}

fn get_maybe_create_repo_sync_target_fut(
    ctx: CoreContext,
    incomplete_repo_handler: &IncompleteRepoHandler,
    repo_sync_target_args: RepoSyncTargetArgs,
    lookup_table: &HashMap<RepositoryId, IncompleteRepoHandler>,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<Option<RepoSyncTarget>, Error> {
    let large_repo_id = repo_sync_target_args.commit_sync_config.large_repo_id;
    let current_repo_id = incomplete_repo_handler.repo.repoid();
    let current_repo = &incomplete_repo_handler.repo;
    let target_incomplete_repo_handler = try_boxfuture!(lookup_table
        .get(&large_repo_id)
        .ok_or(ErrorKind::LargeRepoNotFound(large_repo_id)));

    if large_repo_id == current_repo_id {
        future::ok(None).boxify()
    } else {
        create_repo_sync_target(
            ctx,
            current_repo,
            target_incomplete_repo_handler,
            repo_sync_target_args,
            readonly_storage,
        )
        .map(Some)
        .boxify()
    }
}

pub fn repo_handlers(
    fb: FacebookInit,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
    caching: Caching,
    disabled_hooks: &HashSet<String>,
    scuba_censored_table: Option<String>,
    readonly_storage: ReadOnlyStorage,
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
                Option<RepoSyncTargetArgs>,
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
            info!(
                root_log,
                "Start warming for repo {}, type {:?}", reponame, config.storage_config.blobstore
            );
            let root_log = root_log.clone();
            let logger = root_log.new(o!("repo" => reponame.clone()));
            let ctx = CoreContext::new_with_logger(fb, logger.clone());

            let ready_handle = ready.create_handle(reponame.clone());

            let repoid = config.repoid;
            let disabled_hooks = disabled_hooks.clone();

            open_blobrepo(
                fb,
                config.storage_config.clone(),
                repoid,
                myrouter_port,
                caching,
                config.bookmarks_cache_ttl,
                config.redaction,
                scuba_censored_table.clone(),
                config.filestore.clone(),
                readonly_storage,
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
                    hash_validation_percentage,
                    wireproto_logging,
                    bundle2_replay_params,
                    push,
                    hook_max_file_size,
                    commit_sync_config,
                    ..
                } = config.clone();

                let hook_manager_params = hook_manager_params.unwrap_or(Default::default());

                let mut hook_manager = HookManager::new(
                    ctx.clone(),
                    Box::new(BlobRepoChangesetStore::new(blobrepo.clone())),
                    blobrepo_text_only_store(blobrepo.clone(), hook_max_file_size),
                    hook_manager_params,
                );

                // TODO: Don't require full config in load_hooks so we can avoid a clone here.
                info!(root_log, "Loading hooks");
                try_boxfuture!(load_hooks(
                    fb,
                    &mut hook_manager,
                    config.clone(),
                    &disabled_hooks
                ));

                let streaming_clone = if let Some(db_address) = dbconfig.get_db_address() {
                    streaming_clone(
                        blobrepo.clone(),
                        db_address,
                        myrouter_port,
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
                    SqlRepoReadWriteStatus::with_xdb(addr, myrouter_port, readonly_storage.0)
                        .map(Some)
                        .left_future()
                } else {
                    Ok(None).into_future().right_future()
                };

                let sql_mutable_counters = open_db_from_config::<SqlMutableCounters>(
                    &dbconfig,
                    myrouter_port,
                    readonly_storage,
                );

                let phases_hint =
                    open_db_from_config::<SqlPhases>(&dbconfig, myrouter_port, readonly_storage);

                let sql_commit_sync_mapping = open_db_from_config::<SqlSyncedCommitMapping>(
                    &dbconfig,
                    myrouter_port,
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

                            let initial_warmup = cache_warmup(
                                ctx.clone(),
                                blobrepo.clone(),
                                cache_warmup_params,
                                listen_log.clone(),
                            )
                            .chain_err(format!("while warming up cache for repo: {}", reponame))
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

                            ready_handle
                                .wait_for(
                                    initial_warmup
                                        .and_then(|()| skip_index.join(support_bundle2_listkeys)),
                                )
                                .map({
                                    cloned!(root_log);
                                    move |(skip_index, support_bundle2_listkeys)| {
                                        info!(root_log, "Repo warmup for {} complete", reponame);

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

                                        let maybe_repo_sync_target_args =
                                            commit_sync_config.map(move |commit_sync_config| {
                                                RepoSyncTargetArgs {
                                                    commit_sync_config,
                                                    synced_commit_mapping: sql_commit_sync_mapping,
                                                    db_config: dbconfig,
                                                    maybe_myrouter_port: myrouter_port,
                                                }
                                            });

                                        let wireproto_logging = wireproto_logging.map(|config| {
                                            Arc::new(WireprotoLogging::new(
                                                fb,
                                                reponame.clone(),
                                                config,
                                            ))
                                        });

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
                                            maybe_repo_sync_target_args,
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
        Option<RepoSyncTargetArgs>,
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
            move |(ctx, reponame, incomplete_repo_handler, maybe_repo_sync_target_args)| {
                let maybe_repo_sync_target_fut = match maybe_repo_sync_target_args {
                    None => future::ok(None).boxify(),
                    Some(repo_sync_target_args) => get_maybe_create_repo_sync_target_fut(
                        ctx.clone(),
                        &incomplete_repo_handler,
                        repo_sync_target_args,
                        &lookup_table,
                        readonly_storage,
                    ),
                };

                maybe_repo_sync_target_fut
                    .map(move |maybe_repo_sync_target| {
                        (
                            reponame,
                            incomplete_repo_handler
                                .into_repo_handler_with_sync_target(maybe_repo_sync_target),
                        )
                    })
                    .boxify()
            },
        )
    })
    .map(|v| v.into_iter().collect())
}
