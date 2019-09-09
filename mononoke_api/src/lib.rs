// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(async_await)]

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
use blobrepo::BlobRepo;
use blobrepo_factory::Caching;
use cloned::cloned;
use failure::Error;
use futures_preview::future;
use slog::{debug, info, o, Logger};

use metaconfig_parser::RepoConfigs;

use crate::repo::Repo;

pub mod changeset;
pub mod errors;
pub mod legacy;
pub mod repo;
pub mod specifiers;

#[cfg(test)]
mod test;

pub use crate::legacy::get_content_by_path;

pub use crate::changeset::ChangesetContext;
pub use crate::errors::MononokeError;
pub use crate::repo::RepoContext;
pub use crate::specifiers::{ChangesetId, ChangesetSpecifier, HgChangesetId};

// Re-export types that are useful for clients.
pub type CoreContext = context::CoreContext;

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(
        logger: Logger,
        configs: RepoConfigs,
        myrouter_port: Option<u16>,
        with_cachelib: Caching,
    ) -> Result<Self, Error> {
        let common_config = configs.common;
        let repos = future::join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map(move |(name, config)| {
                    cloned!(logger, common_config);
                    async move {
                        info!(logger, "Initializing repo: {}", &name);
                        let repo = Repo::new(
                            logger.new(o!("repo" => name.clone())),
                            config,
                            common_config,
                            myrouter_port,
                            with_cachelib,
                        )
                        .await
                        .expect("failed to initialize repo");
                        debug!(logger, "Initialized {}", &name);
                        (name, Arc::new(repo))
                    }
                }),
        )
        .await
        .into_iter()
        .collect();
        Ok(Self { repos })
    }

    /// Create a Mononoke instance for testing.
    #[cfg(test)]
    fn new_test(repos: impl IntoIterator<Item = (String, BlobRepo)>) -> Self {
        Self {
            repos: repos
                .into_iter()
                .map(|(name, repo)| (name, Arc::new(Repo::new_test(repo))))
                .collect(),
        }
    }

    /// Start a request on a repository.
    pub fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContext>, MononokeError> {
        let name = name.as_ref();
        let repo = self.repos.get(name).map(move |repo| RepoContext {
            repo: repo.clone(),
            ctx,
        });
        Ok(repo)
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.repos.keys().map(AsRef::as_ref)
    }
}
