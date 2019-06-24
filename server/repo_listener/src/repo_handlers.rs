// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use crate::failure::prelude::*;
use futures::{
    future::{self, ok},
    Future,
};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use blobrepo_factory::{open_blobrepo, Caching};
use blobstore::Blobstore;
use cache_warmup::cache_warmup;
use context::CoreContext;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{BlobRepoChangesetStore, BlobRepoFileContentStore};
use metaconfig_types::{MetadataDBConfig, RepoConfig, StorageConfig};
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use phases::{CachingPhases, Phases, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use ready_state::ReadyStateBuilder;
use repo_client::{streaming_clone, MononokeRepo, RepoReadWriteFetcher};
use repo_read_write_status::SqlRepoReadWriteStatus;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use skiplist::{deserialize_skiplist_map, SkiplistIndex};
use sql_ext::SqlConstructors;

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_scribe_category: Option<String>,
    pub repo: MononokeRepo,
    pub hash_validation_percentage: usize,
    pub lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    pub phases_hint: Arc<dyn Phases>,
    pub preserve_raw_bundle2: bool,
    pub pure_push_allowed: bool,
    pub support_bundle2_listkeys: bool,
}

fn open_db_from_config<S: SqlConstructors>(
    dbconfig: &MetadataDBConfig,
    myrouter_port: Option<u16>,
) -> Result<S> {
    match dbconfig {
        MetadataDBConfig::LocalDB { ref path } => S::with_sqlite_path(path.join(S::LABEL)),
        MetadataDBConfig::Mysql { ref db_address, .. } => match myrouter_port {
            Some(port) => Ok(S::with_myrouter(&db_address, port)),
            None => S::with_raw_xdb_tier(&db_address),
        },
    }
}

pub fn repo_handlers(
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
    caching: Caching,
    root_log: &Logger,
    ready: &mut ReadyStateBuilder,
) -> BoxFuture<HashMap<String, RepoHandler>, Error> {
    // compute eagerly to avoid lifetime issues
    let repos: Vec<_> = repos
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
            // TODO(T37478150, luk): this is not a test use case, need to address this later
            let ctx = CoreContext::test_mock();

            let ready_handle = ready.create_handle(reponame.clone());

            let root_log = root_log.clone();
            let logger = root_log.new(o!("repo" => reponame.clone()));
            let repoid = RepositoryId::new(config.repoid);
            open_blobrepo(
                logger.clone(),
                config.storage_config.clone(),
                repoid,
                myrouter_port,
                caching,
                config.bookmarks_cache_ttl,
            )
            .and_then(move |blobrepo| {
                let hook_manager_params = match config.hook_manager_params.clone() {
                    Some(hook_manager_params) => hook_manager_params,
                    None => Default::default(),
                };

                let mut hook_manager = HookManager::new(
                    ctx.clone(),
                    Box::new(BlobRepoChangesetStore::new(blobrepo.clone())),
                    Arc::new(BlobRepoFileContentStore::new(blobrepo.clone())),
                    hook_manager_params,
                    logger,
                );

                info!(root_log, "Loading hooks");
                try_boxfuture!(load_hooks(&mut hook_manager, config.clone()));

                let streaming_clone =
                    if let Some(db_address) = config.storage_config.dbconfig.get_db_address() {
                        Some(try_boxfuture!(streaming_clone(
                            blobrepo.clone(),
                            &db_address,
                            myrouter_port,
                            repoid
                        )))
                    } else {
                        None
                    };

                // XXX Fixme - put write_lock_db_address into storage_config.dbconfig?
                let read_write_fetcher = if let Some(addr) = config.write_lock_db_address {
                    let sql_repo_read_write_status =
                        SqlRepoReadWriteStatus::with_xdb(addr.clone(), myrouter_port).unwrap();
                    RepoReadWriteFetcher::new(
                        Some(sql_repo_read_write_status),
                        config.readonly.clone(),
                        reponame.clone(),
                    )
                } else {
                    RepoReadWriteFetcher::new(None, config.readonly.clone(), reponame.clone())
                };

                let repo = MononokeRepo::new(
                    blobrepo,
                    &config.pushrebase,
                    config.bookmarks.clone(),
                    Arc::new(hook_manager),
                    streaming_clone,
                    config.lfs.clone(),
                    reponame.clone(),
                    read_write_fetcher,
                    config.infinitepush,
                    config.list_keys_patterns_max,
                );

                let listen_log = root_log.new(o!("repo" => reponame.clone()));
                let mut scuba_logger =
                    ScubaSampleBuilder::with_opt_table(config.scuba_table.clone());
                scuba_logger.add_common_server_data();
                let hash_validation_percentage = config.hash_validation_percentage.clone();
                let wireproto_scribe_category = config.wireproto_scribe_category.clone();
                let preserve_raw_bundle2 =
                    config.bundle2_replay_params.preserve_raw_bundle2.clone();
                let pure_push_allowed = config.push.pure_push_allowed.clone();

                let skip_index = match config.skiplist_index_blobstore_key.clone() {
                    Some(skiplist_index_blobstore_key) => {
                        let blobstore = repo.blobrepo().get_blobstore();
                        blobstore
                            .get(ctx.clone(), skiplist_index_blobstore_key)
                            .and_then(|maybebytes| {
                                let map = match maybebytes {
                                    Some(bytes) => {
                                        let bytes = bytes.into_bytes();
                                        try_boxfuture!(deserialize_skiplist_map(bytes))
                                    }
                                    None => HashMap::new(),
                                };
                                ok(Arc::new(SkiplistIndex::new_with_skiplist_graph(map))).boxify()
                            })
                            .left_future()
                    }
                    None => ok(Arc::new(SkiplistIndex::new())).right_future(),
                };

                let RepoConfig {
                    storage_config: StorageConfig { dbconfig, .. },
                    cache_warmup: cache_warmup_params,
                    ..
                } = config;

                let initial_warmup = cache_warmup(
                    ctx.clone(),
                    repo.blobrepo().clone(),
                    cache_warmup_params,
                    listen_log.clone(),
                )
                .chain_err(format!("while warming up cache for repo: {}", reponame))
                .from_err();

                // TODO: T45466266 this should be replaced by gatekeepers
                let support_bundle2_listkeys = try_boxfuture!(open_db_from_config::<
                    SqlMutableCounters,
                >(
                    &dbconfig, myrouter_port
                ))
                .get_counter(
                    ctx.clone(),
                    repo.blobrepo().get_repoid(),
                    "support_bundle2_listkeys",
                )
                .map(|val| val.unwrap_or(1) != 0);

                ready_handle
                    .wait_for(
                        initial_warmup.and_then(|()| skip_index.join(support_bundle2_listkeys)),
                    )
                    .and_then({
                        cloned!(root_log);
                        move |(skip_index, support_bundle2_listkeys)| {
                            info!(root_log, "Repo warmup for {} complete", reponame);

                            // initialize phases hint from the skip index
                            let phases_hint =
                                open_db_from_config::<SqlPhases>(&dbconfig, myrouter_port).map(
                                    |db| -> Arc<dyn Phases> {
                                        let db = Arc::new(db);

                                        if let MetadataDBConfig::Mysql { .. } = dbconfig {
                                            Arc::new(CachingPhases::new(db))
                                        } else {
                                            db
                                        }
                                    },
                                );

                            // initialize lca hint from the skip index
                            let lca_hint: Arc<dyn LeastCommonAncestorsHint> = skip_index;

                            Ok((
                                reponame,
                                RepoHandler {
                                    logger: listen_log,
                                    scuba: scuba_logger,
                                    wireproto_scribe_category,
                                    repo,
                                    hash_validation_percentage,
                                    lca_hint,
                                    phases_hint: phases_hint?,
                                    preserve_raw_bundle2,
                                    pure_push_allowed,
                                    support_bundle2_listkeys,
                                },
                            ))
                        }
                    })
                    .boxify()
            })
            .boxify()
        })
        .collect();

    future::join_all(repos)
        .map(|repos| repos.into_iter().collect())
        .boxify()
}
