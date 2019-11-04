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

use blobrepo_factory::{open_blobrepo, Caching};
use bookmark_renaming::{get_large_to_small_renamer, get_small_to_large_renamer};
use cache_warmup::cache_warmup;
use context::CoreContext;
use cross_repo_sync::{CommitSyncRepos, CommitSyncer};
use fbinit::FacebookInit;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{blobrepo_text_only_store, BlobRepoChangesetStore};
use metaconfig_types::{
    CommitSyncConfig, CommitSyncDirection, MetadataDBConfig, RepoConfig, StorageConfig,
    WireprotoLoggingConfig,
};
use mononoke_types::RepositoryId;
use movers::{get_large_to_small_mover, get_small_to_large_mover};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use phases::{CachingPhases, Phases, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use ready_state::ReadyStateBuilder;
use repo_client::{streaming_clone, MononokeRepo, RepoReadWriteFetcher, RepoSyncTarget};
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
    wireproto_logging: Option<WireprotoLoggingConfig>,
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

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_logging: Option<WireprotoLoggingConfig>,
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
) -> BoxFuture<S, Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { ref path } => S::with_sqlite_path(path.join(S::LABEL))
            .into_future()
            .boxify(),
        MetadataDBConfig::Mysql { ref db_address, .. } => {
            S::with_xdb(db_address.clone(), myrouter_port)
        }
    }
}

/// Given a `CommitSyncConfig`, a small repo id, and an
/// auxillary struct, that holds partially built `RepoHandler`,
/// build `RepoSyncTarget` for a push rediction from this
/// small repo into a large repo.
fn create_repo_sync_target(
    source_repo: &MononokeRepo,
    target_incomplete_repo_handler: &IncompleteRepoHandler,
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: RepositoryId,
    synced_commit_mapping: SqlSyncedCommitMapping,
) -> Result<RepoSyncTarget> {
    let small_to_large_mover = get_small_to_large_mover(commit_sync_config, small_repo_id)?;
    let large_to_small_mover = get_large_to_small_mover(commit_sync_config, small_repo_id)?;
    let small_to_large_renamer = get_small_to_large_renamer(commit_sync_config, small_repo_id)?;
    let large_to_small_renamer = get_large_to_small_renamer(commit_sync_config, small_repo_id)?;

    let small_repo = source_repo.blobrepo().clone();
    let large_repo = target_incomplete_repo_handler.repo.blobrepo().clone();

    let small_to_large_commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: small_repo.clone(),
        large_repo: large_repo.clone(),
        mover: small_to_large_mover.clone(),
        bookmark_renamer: small_to_large_renamer.clone(),
    };

    let large_to_small_commit_sync_repos = CommitSyncRepos::LargeToSmall {
        small_repo,
        large_repo,
        mover: large_to_small_mover,
        bookmark_renamer: large_to_small_renamer,
    };

    let mapping: Arc<dyn SyncedCommitMapping> = Arc::new(synced_commit_mapping);

    let small_to_large_commit_syncer = CommitSyncer {
        mapping: mapping.clone(),
        repos: small_to_large_commit_sync_repos,
    };
    let large_to_small_commit_syncer = CommitSyncer {
        mapping,
        repos: large_to_small_commit_sync_repos,
    };

    let repo = target_incomplete_repo_handler.repo.clone();

    Ok(RepoSyncTarget {
        repo,
        small_to_large_commit_syncer,
        large_to_small_commit_syncer,
    })
}

pub fn repo_handlers(
    fb: FacebookInit,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
    caching: Caching,
    disabled_hooks: &HashSet<String>,
    scuba_censored_table: Option<String>,
    root_log: &Logger,
    ready: &mut ReadyStateBuilder,
) -> BoxFuture<HashMap<String, RepoHandler>, Error> {
    // compute eagerly to avoid lifetime issues
    let repo_futs: Vec<
        BoxFuture<
            (
                String,
                IncompleteRepoHandler,
                Option<CommitSyncConfig>,
                SqlSyncedCommitMapping,
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
                    streaming_clone(blobrepo.clone(), db_address, myrouter_port, repoid)
                        .map(Some)
                        .left_future()
                } else {
                    Ok(None).into_future().right_future()
                };

                // XXX Fixme - put write_lock_db_address into storage_config.dbconfig?
                let sql_read_write_status = if let Some(addr) = write_lock_db_address {
                    SqlRepoReadWriteStatus::with_xdb(addr, myrouter_port)
                        .map(Some)
                        .left_future()
                } else {
                    Ok(None).into_future().right_future()
                };

                let sql_mutable_counters =
                    open_db_from_config::<SqlMutableCounters>(&dbconfig, myrouter_port);

                let phases_hint = open_db_from_config::<SqlPhases>(&dbconfig, myrouter_port);

                let sql_commit_sync_mapping =
                    open_db_from_config::<SqlSyncedCommitMapping>(&dbconfig, myrouter_port);

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
                                            if let MetadataDBConfig::Mysql { .. } = dbconfig {
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

                                        (
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
                                            commit_sync_config,
                                            sql_commit_sync_mapping,
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
        .and_then(build_repo_handlers)
        .boxify()
}

fn build_repo_handlers(
    tuples: Vec<(
        String,
        IncompleteRepoHandler,
        Option<CommitSyncConfig>,
        SqlSyncedCommitMapping,
    )>,
) -> Result<HashMap<String, RepoHandler>> {
    let lookup_table: HashMap<RepositoryId, IncompleteRepoHandler> = tuples
        .clone()
        .into_iter()
        .map(|(_, incomplete_repo_handler, _, _)| {
            (
                incomplete_repo_handler.repo.repoid(),
                incomplete_repo_handler,
            )
        })
        .collect();

    tuples
        .into_iter()
        .map(
            |(reponame, incomplete_repo_handler, maybe_commit_sync_config, commit_sync_mapping)| -> Result<(String, RepoHandler)> {
                let maybe_repo_sync_target = match maybe_commit_sync_config {
                    None => None,
                    Some(commit_sync_config) => {
                        let large_repo_id = commit_sync_config.large_repo_id;
                        let current_repo_id = incomplete_repo_handler.repo.repoid();
                        let current_repo = &incomplete_repo_handler.repo;

                        if large_repo_id == current_repo_id {
                            None
                        } else {
                            let direction = commit_sync_config
                                .small_repos
                                .get(&current_repo_id)
                                .ok_or(ErrorKind::SmallRepoNotFound(current_repo_id))?
                                .direction;
                            if direction != CommitSyncDirection::LargeToSmall {
                                // We can only do push redirection when sync happens in the
                                // `LargeToSmall` direction, as `SmallToLarge` is handled by
                                // tailers.
                                None
                            } else {
                                let target_incomplete_repo_handler = lookup_table
                                    .get(&large_repo_id)
                                    .ok_or(ErrorKind::LargeRepoNotFound(large_repo_id))?;
                                Some(create_repo_sync_target(
                                    current_repo,
                                    target_incomplete_repo_handler,
                                    &commit_sync_config,
                                    current_repo_id,
                                    commit_sync_mapping,
                                )?)
                            }
                        }
                    }
                };

                Ok((
                    reponame,
                    incomplete_repo_handler.into_repo_handler_with_sync_target(maybe_repo_sync_target),
                ))
            },
        )
        .collect::<Result<HashMap<_, _>>>()
}
