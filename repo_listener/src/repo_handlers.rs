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

use cache_warmup::cache_warmup;
use mercurial_types::RepositoryId;
use metaconfig::repoconfig::RepoConfig;
use ready_state::ReadyStateBuilder;
use repo_client::MononokeRepo;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};

pub type RepoHandler = (Logger, ScubaSampleBuilder, Arc<MononokeRepo>);

pub fn repo_handlers<I>(
    repos: I,
    root_log: &Logger,
    ready: &mut ReadyStateBuilder,
) -> BoxFuture<HashMap<String, RepoHandler>, Error>
where
    I: IntoIterator<Item = (String, RepoConfig)>,
{
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
            info!(root_log, "Start listening for repo {:?}", config.repotype);
            let ready_handle = ready.create_handle(reponame.as_ref());

            let repo = MononokeRepo::new(
                root_log.new(o!("repo" => reponame.clone())),
                &config.repotype,
                RepositoryId::new(config.repoid),
            ).expect(&format!("failed to initialize repo {}", reponame));

            let listen_log = root_log.new(o!("repo" => repo.path().clone()));

            let scuba_logger = ScubaSampleBuilder::with_opt_table(config.scuba_table.clone());

            let repo = Arc::new(repo);

            let initial_warmup =
                cache_warmup(repo.blobrepo(), config.cache_warmup, listen_log.clone())
                    .context(format!("while warming up cache for repo: {}", reponame))
                    .from_err();
            ready_handle
                .wait_for(initial_warmup)
                .map(move |()| (reponame, (listen_log, scuba_logger, repo)))
        })
        .collect();

    future::join_all(repos)
        .map(|repos| repos.into_iter().collect())
        .boxify()
}
