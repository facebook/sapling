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
use sql::myrouter;

use blobrepo_factory::open_blobrepo;
use blobstore::Blobstore;
use cache_warmup::cache_warmup;
use context::CoreContext;
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{BlobRepoChangesetStore, BlobRepoFileContentStore};
use metaconfig_types::{MetadataDBConfig, RepoConfig, StorageConfig};
use mononoke_types::RepositoryId;
use phases::{CachingPhases, Phases, SqlConstructors, SqlPhases};
use reachabilityindex::LeastCommonAncestorsHint;
use ready_state::ReadyStateBuilder;
use repo_client::{streaming_clone, MononokeRepo, RepoReadWriteFetcher};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use skiplist::{deserialize_skiplist_map, SkiplistIndex};

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
}

pub fn repo_handlers(
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
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
            let ensure_myrouter_ready = match config.storage_config.dbconfig.get_db_address() {
                None => future::ok(()).left_future(),
                Some(db_address) => {
                    let myrouter_port = try_boxfuture!(myrouter_port.ok_or_else(|| format_err!(
                        "No port for MyRouter provided, but repo {} needs to connect do db {}",
                        reponame,
                        db_address
                    )));
                    myrouter::wait_for_myrouter(myrouter_port, db_address).right_future()
                }
            };

            let ready_handle = ready.create_handle(reponame.clone());

            let root_log = root_log.clone();
            let logger = root_log.new(o!("repo" => reponame.clone()));
            let repoid = RepositoryId::new(config.repoid);
            open_blobrepo(
                logger.clone(),
                config.storage_config.clone(),
                repoid,
                myrouter_port,
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
                            myrouter_port.expect("myrouter_port not provided for BlobRemote repo"),
                            repoid
                        )))
                    } else {
                        None
                    };

                // XXX Fixme - put write_lock_db_address into storage_config.dbconfig?
                let read_write_fetcher = if let Some(addr) = config.write_lock_db_address {
                    RepoReadWriteFetcher::with_myrouter(
                        config.readonly.clone(),
                        reponame.clone(),
                        addr.clone(),
                        myrouter_port.expect("myrouter_port not provided for BlobRemote repo"),
                    )
                } else {
                    RepoReadWriteFetcher::new(config.readonly.clone(), reponame.clone())
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

                // TODO (T32873881): Arc<BlobRepo> should become BlobRepo
                let initial_warmup = ensure_myrouter_ready.and_then({
                    cloned!(ctx, reponame, listen_log);
                    let blobrepo = repo.blobrepo().clone();
                    move |()| {
                        cache_warmup(ctx, blobrepo, cache_warmup_params, listen_log)
                            .chain_err(format!("while warming up cache for repo: {}", reponame))
                            .from_err()
                    }
                });

                ready_handle
                    .wait_for(initial_warmup.and_then(|()| skip_index))
                    .map({
                        cloned!(root_log);
                        move |skip_index| {
                            info!(root_log, "Repo warmup for {} complete", reponame);

                            // initialize phases hint from the skip index
                            let phases_hint: Arc<dyn Phases> = match dbconfig {
                                MetadataDBConfig::LocalDB { path } => Arc::new(
                                    SqlPhases::with_sqlite_path(path.join("phases"))
                                        .expect("unable to initialize sqlite db for phases"),
                                ),
                                MetadataDBConfig::Mysql { db_address, .. } => {
                                    let storage = Arc::new(SqlPhases::with_myrouter(
                                        &db_address,
                                        myrouter_port.expect(
                                            "myrouter_port not provided for BlobRemote repo",
                                        ),
                                    ));
                                    Arc::new(CachingPhases::new(storage))
                                }
                            };

                            // initialize lca hint from the skip index
                            let lca_hint: Arc<dyn LeastCommonAncestorsHint> = skip_index;

                            (
                                reponame,
                                RepoHandler {
                                    logger: listen_log,
                                    scuba: scuba_logger,
                                    wireproto_scribe_category,
                                    repo,
                                    hash_validation_percentage,
                                    lca_hint,
                                    phases_hint,
                                    preserve_raw_bundle2,
                                },
                            )
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
