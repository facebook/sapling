/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use clap::{ArgMatches, Error as ClapError, FromArgMatches};
use context::CoreContext;
use environment::MononokeEnvironment;
use facet::AsyncBuildable;
use fbinit::FacebookInit;
use metaconfig_parser::{RepoConfigs, StorageConfigs};
use mononoke_args::config::ConfigArgs;
use mononoke_args::repo::{RepoArg, RepoArgs};
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use slog::Logger;
use tokio::runtime::Handle;

pub struct MononokeApp {
    args: ArgMatches,
    env: Arc<MononokeEnvironment>,
    storage_configs: StorageConfigs,
    repo_configs: RepoConfigs,
    repo_factory: RepoFactory,
}

impl MononokeApp {
    pub(crate) fn new(
        _fb: FacebookInit,
        args: ArgMatches,
        env: MononokeEnvironment,
    ) -> Result<Self> {
        let env = Arc::new(env);
        let config_path = ConfigArgs::from_arg_matches(&args)?.config_path();

        let config_store = &env.as_ref().config_store;
        let storage_configs = metaconfig_parser::load_storage_configs(&config_path, config_store)?;
        let repo_configs = metaconfig_parser::load_repo_configs(&config_path, config_store)?;

        let repo_factory = RepoFactory::new(env.clone(), &repo_configs.common);

        Ok(MononokeApp {
            args,
            env,
            storage_configs,
            repo_configs,
            repo_factory,
        })
    }

    /// Execute a future on this app's runtime.
    pub fn run<F, Fut>(self, main: F) -> Result<()>
    where
        F: Fn(MononokeApp) -> Fut,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let env = self.env.clone();
        env.runtime
            .block_on(async move { tokio::spawn(main(self)).await? })
    }

    /// Returns the selected subcommand of the app (if this app
    /// has subcommands).
    pub fn subcommand(&self) -> Option<(&str, &ArgMatches)> {
        self.args.subcommand()
    }

    /// Returns a parsed args struct based on the arguments provided
    /// on the command line.
    pub fn args<Args>(&self) -> Result<Args, ClapError>
    where
        Args: FromArgMatches,
    {
        Args::from_arg_matches(&self.args)
    }

    /// Returns a handle to this app's runtime.
    pub fn runtime(&self) -> &Handle {
        self.env.runtime.handle()
    }

    /// The repo configs for this app.
    pub fn repo_configs(&self) -> &RepoConfigs {
        &self.repo_configs
    }

    /// The storage configs for this app.
    pub fn storage_configs(&self) -> &StorageConfigs {
        &self.storage_configs
    }

    /// The logger for this app.
    pub fn logger(&self) -> &Logger {
        &self.env.logger
    }

    /// Create a basic CoreContext.
    pub fn new_context(&self) -> CoreContext {
        CoreContext::new_with_logger(self.env.fb, self.logger().clone())
    }

    /// Open a repository based on user-provided arguments.
    pub async fn open_repo<Repo>(&self, repo_args: &RepoArgs) -> Result<Repo>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let (repo_name, repo_config) = match repo_args.id_or_name()? {
            RepoArg::Id(repo_id) => {
                let (repo_name, repo_config) = self
                    .repo_configs
                    .get_repo_config(repo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown repoid: {:?}", repo_id))?;
                (repo_name.clone(), repo_config.clone())
            }
            RepoArg::Name(repo_name) => {
                let repo_config = self
                    .repo_configs
                    .repos
                    .get(repo_name)
                    .ok_or_else(|| anyhow::anyhow!("unknown reponame: {:?}", repo_name))?;
                (repo_name.to_string(), repo_config.clone())
            }
        };

        let repo = self.repo_factory.build(repo_name, repo_config).await?;

        Ok(repo)
    }
}
