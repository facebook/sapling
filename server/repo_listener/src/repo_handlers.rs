// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use failure::prelude::*;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;
use sql::myrouter;

use cache_warmup::cache_warmup;
use hooks::{HookManager, hook_loader::load_hooks};
use mercurial_types::RepositoryId;
use metaconfig::repoconfig::{RepoConfig, RepoType};
use ready_state::ReadyStateBuilder;
use repo_client::{open_blobrepo, streaming_clone, MononokeRepo};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};

#[derive(Clone, Debug)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_scribe_category: Option<String>,
    pub repo: MononokeRepo,
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
                "Start warming for repo {}, type {:?}", reponame, config.repotype
            );
            let ensure_myrouter_ready = match config.get_db_address() {
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

            let ready_handle = ready.create_handle(reponame.as_ref());

            let logger = root_log.new(o!("repo" => reponame.clone()));
            let repoid = RepositoryId::new(config.repoid);
            let blobrepo = try_boxfuture!(open_blobrepo(
                logger.clone(),
                config.repotype.clone(),
                repoid,
                myrouter_port,
            ));

            let mut hook_manager = HookManager::new_with_blobrepo(blobrepo.clone(), logger);

            info!(root_log, "Loading hooks");
            try_boxfuture!(load_hooks(&mut hook_manager, config.clone()));

            let streaming_clone = match config.repotype {
                RepoType::BlobManifold(ref args) => Some(try_boxfuture!(streaming_clone(
                    blobrepo.clone(),
                    &args.db_address,
                    myrouter_port.expect("myrouter_port not provided for BlobManifold repo"),
                    repoid
                ))),
                _ => None,
            };

            let repo = MononokeRepo::new(
                blobrepo,
                &config.pushrebase,
                Arc::new(hook_manager),
                streaming_clone,
                config.lfs.clone(),
            );

            let listen_log = root_log.new(o!("repo" => reponame.clone()));
            let mut scuba_logger = ScubaSampleBuilder::with_opt_table(config.scuba_table.clone());
            scuba_logger.add_common_server_data();
            let wireproto_scribe_category = config.wireproto_scribe_category.clone();

            // TODO (T32873881): Arc<BlobRepo> should become BlobRepo
            let initial_warmup = ensure_myrouter_ready.and_then({
                cloned!(reponame, listen_log);
                let blobrepo = repo.blobrepo().clone();
                move |()| {
                    cache_warmup(Arc::new(blobrepo), config.cache_warmup, listen_log)
                        .chain_err(format!("while warming up cache for repo: {}", reponame))
                        .from_err()
                }
            });
            ready_handle
                .wait_for(initial_warmup)
                .map({
                    cloned!(root_log);
                    move |()| {
                        info!(root_log, "Repo warmup for {} complete", reponame);
                        (
                            reponame,
                            RepoHandler {
                                logger: listen_log,
                                scuba: scuba_logger,
                                wireproto_scribe_category,
                                repo: repo,
                            },
                        )
                    }
                })
                .boxify()
        })
        .collect();

    future::join_all(repos)
        .map(|repos| repos.into_iter().collect())
        .boxify()
}
