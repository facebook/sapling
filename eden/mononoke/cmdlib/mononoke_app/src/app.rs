/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::TypeId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use base_app::BaseApp;
use blobstore::Blobstore;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use clap::ArgMatches;
use clap::Error as ClapError;
use clap::FromArgMatches;
use context::CoreContext;
use environment::MononokeEnvironment;
use facet::AsyncBuildable;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use redactedblobstore::RedactedBlobstore;
use redactedblobstore::RedactedBlobstoreConfig;
use redactedblobstore::RedactionConfigBlobstore;
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::o;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use tokio::runtime::Handle;

use crate::args::ConfigArgs;
use crate::args::ConfigMode;
use crate::args::MultiRepoArgs;
use crate::args::RepoArg;
use crate::args::RepoArgs;
use crate::args::RepoBlobstoreArgs;
use crate::extension::AppExtension;
use crate::extension::AppExtensionArgsBox;
use crate::extension::BoxedAppExtensionArgs;

pub struct MononokeApp {
    pub fb: FacebookInit,
    config_mode: ConfigMode,
    args: ArgMatches,
    env: Arc<MononokeEnvironment>,
    extension_args: HashMap<TypeId, Box<dyn BoxedAppExtensionArgs>>,
    storage_configs: StorageConfigs,
    repo_configs: RepoConfigs,
    repo_factory: RepoFactory,
}

impl BaseApp for MononokeApp {
    fn subcommand(&self) -> Option<(&str, &ArgMatches)> {
        self.args.subcommand()
    }
}

impl MononokeApp {
    pub(crate) fn new(
        fb: FacebookInit,
        config_mode: ConfigMode,
        args: ArgMatches,
        env: MononokeEnvironment,
        extension_args: HashMap<TypeId, Box<dyn BoxedAppExtensionArgs>>,
    ) -> Result<Self> {
        let env = Arc::new(env);
        let config_path = ConfigArgs::from_arg_matches(&args)?.config_path();

        let config_store = &env.as_ref().config_store;
        let storage_configs = metaconfig_parser::load_storage_configs(&config_path, config_store)?;
        let repo_configs = metaconfig_parser::load_repo_configs(&config_path, config_store)?;

        let repo_factory = RepoFactory::new(env.clone(), &repo_configs.common);

        Ok(MononokeApp {
            fb,
            config_mode,
            args,
            env,
            extension_args,
            storage_configs,
            repo_configs,
            repo_factory,
        })
    }

    pub fn extension_args<Ext>(&self) -> Result<&Ext::Args>
    where
        Ext: AppExtension + 'static,
    {
        if let Some(ext) = self.extension_args.get(&TypeId::of::<Ext>()) {
            if let Some(ext) = ext.as_any().downcast_ref::<AppExtensionArgsBox<Ext>>() {
                return Ok(ext.args());
            }
        }
        Err(anyhow!(
            "Extension {} arguments not found (was it registered with MononokeApp?)",
            std::any::type_name::<Ext>(),
        ))
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
    pub fn matches(&self) -> &ArgMatches {
        &self.args
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

    /// The config store for this app.
    pub fn config_store(&self) -> &ConfigStore {
        &self.env.config_store
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

    /// Construct a logger for a specific repo.
    pub fn repo_logger(&self, repo_name: &str) -> Logger {
        self.env.logger.new(o!("repo" => repo_name.to_string()))
    }

    /// The mysql options for this app.
    pub fn mysql_options(&self) -> &MysqlOptions {
        &self.env.mysql_options
    }

    /// The blobstore options for this app.
    pub fn blobstore_options(&self) -> &BlobstoreOptions {
        &self.env.blobstore_options
    }

    /// The readonly storage options for this app.
    pub fn readonly_storage(&self) -> &ReadOnlyStorage {
        &self.env.readonly_storage
    }

    /// Create a basic CoreContext.
    pub fn new_context(&self) -> CoreContext {
        CoreContext::new_with_logger(self.env.fb, self.logger().clone())
    }

    /// Return repo factory used by app.
    pub fn repo_factory(&self) -> RepoFactory {
        self.repo_factory.clone()
    }

    /// Mononoke environment for this app.
    pub fn environment(&self) -> &Arc<MononokeEnvironment> {
        &self.env
    }

    /// Returns true if this is a production configuration of Mononoke
    pub fn is_production(&self) -> bool {
        self.config_mode == ConfigMode::Production
    }

    /// Get repo config based on user-provided arguments.
    pub fn repo_config(&self, repo_arg: RepoArg) -> Result<(String, RepoConfig)> {
        match repo_arg {
            RepoArg::Id(repo_id) => {
                let (repo_name, repo_config) = self
                    .repo_configs
                    .get_repo_config(repo_id)
                    .ok_or_else(|| anyhow!("unknown repoid: {:?}", repo_id))?;
                Ok((repo_name.clone(), repo_config.clone()))
            }
            RepoArg::Name(repo_name) => {
                let repo_config = self
                    .repo_configs
                    .repos
                    .get(repo_name)
                    .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))?;
                Ok((repo_name.to_string(), repo_config.clone()))
            }
        }
    }

    /// Get repo configs based on user-provided arguments.
    pub fn multi_repo_configs(&self, repo_args: Vec<RepoArg>) -> Result<Vec<(String, RepoConfig)>> {
        let mut repos = vec![];
        let mut unique_repos = HashSet::new();
        for repo in repo_args {
            let (name, repo_conf) = self.repo_config(repo)?;
            if unique_repos.insert(name.clone()) {
                repos.push((name, repo_conf));
            }
        }

        Ok(repos)
    }

    /// Open repositories based on user-provided arguments.
    pub async fn open_repos<Repo>(&self, repos_args: &MultiRepoArgs) -> Result<Vec<Repo>>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let args = repos_args.ids_or_names()?;
        let mut repos = vec![];
        for arg in args {
            repos.push(self.repo_config(arg)?);
        }

        let repos: HashMap<_, _> = repos.into_iter().collect();
        let repos: Vec<_> = stream::iter(repos)
            .map(|(repo_name, repo_config)| {
                let repo_factory = self.repo_factory.clone();
                async move { repo_factory.build(repo_name, repo_config).await }
            })
            .buffered(100)
            .try_collect()
            .await?;

        Ok(repos)
    }

    /// Open a repository based on user-provided arguments.
    pub async fn open_repo<Repo>(&self, repo_args: &RepoArgs) -> Result<Repo>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_arg = repo_args.id_or_name()?;
        let (repo_name, repo_config) = self.repo_config(repo_arg)?;
        let repo = self.repo_factory.build(repo_name, repo_config).await?;
        Ok(repo)
    }

    /// Open just the blobstore based on user-provided arguments.
    pub async fn open_blobstore(
        &self,
        repo_blobstore_args: &RepoBlobstoreArgs,
    ) -> Result<Arc<dyn Blobstore>> {
        let (mut repo_id, redaction, storage_config) =
            if let Some(repo_id) = repo_blobstore_args.repo_id {
                let repo_id = RepositoryId::new(repo_id);
                let (_repo_name, repo_config) = self
                    .repo_configs
                    .get_repo_config(repo_id)
                    .ok_or_else(|| anyhow!("unknown repoid: {:?}", repo_id))?;
                (
                    Some(repo_id),
                    repo_config.redaction,
                    repo_config.storage_config.clone(),
                )
            } else if let Some(repo_name) = &repo_blobstore_args.repo_name {
                let repo_config = self
                    .repo_configs
                    .repos
                    .get(repo_name)
                    .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))?;
                (
                    Some(repo_config.repoid),
                    repo_config.redaction,
                    repo_config.storage_config.clone(),
                )
            } else if let Some(storage_name) = &repo_blobstore_args.storage_name {
                let storage_config = self
                    .storage_configs
                    .storage
                    .get(storage_name)
                    .ok_or_else(|| anyhow!("unknown storage name: {:?}", storage_name))?;
                (None, Redaction::Enabled, storage_config.clone())
            } else {
                return Err(anyhow!("Expected a storage argument"));
            };

        let blob_config = match repo_blobstore_args.inner_blobstore_id {
            None => storage_config.blobstore,
            Some(id) => match storage_config.blobstore {
                BlobConfig::Multiplexed { blobstores, .. } => {
                    let sought_id = BlobstoreId::new(id);
                    blobstores
                        .into_iter()
                        .find_map(|(blobstore_id, _, blobstore)| {
                            if blobstore_id == sought_id {
                                Some(blobstore)
                            } else {
                                None
                            }
                        })
                        .ok_or_else(|| anyhow!("could not find a blobstore with id {}", id))?
                }
                _ => {
                    return Err(anyhow!(
                        "inner-blobstore-id supplied by blobstore is not multiplexed"
                    ));
                }
            },
        };

        if repo_blobstore_args.no_prefix {
            repo_id = None;
        }

        let blobstore = blobstore_factory::make_blobstore(
            self.env.fb,
            blob_config,
            &self.env.mysql_options,
            self.env.readonly_storage,
            &self.env.blobstore_options,
            &self.env.logger,
            &self.env.config_store,
            &blobstore_factory::default_scrub_handler(),
            None,
        )
        .await?;

        let blobstore = if let Some(repo_id) = repo_id {
            PrefixBlobstore::new(blobstore, repo_id.prefix())
        } else {
            PrefixBlobstore::new(blobstore, String::new())
        };

        let blobstore = if redaction == Redaction::Enabled {
            let redacted_blobs = self
                .repo_factory
                .redacted_blobs(self.new_context(), &storage_config.metadata)
                .await?;
            RedactedBlobstore::new(
                blobstore,
                RedactedBlobstoreConfig::new(Some(redacted_blobs), self.redaction_scuba_builder()?),
            )
            .boxed()
        } else {
            Arc::new(blobstore)
        };

        Ok(blobstore)
    }

    pub async fn redaction_config_blobstore(&self) -> Result<Arc<RedactionConfigBlobstore>> {
        self.repo_factory
            .redaction_config_blobstore_from_config(
                &self.repo_configs.common.redaction_config.blobstore,
            )
            .await
    }

    pub async fn redaction_config_blobstore_for_darkstorm(
        &self,
    ) -> Result<Arc<RedactionConfigBlobstore>> {
        let blobstore_config = self
            .repo_configs
            .common
            .redaction_config
            .darkstorm_blobstore
            .as_ref()
            .ok_or_else(|| anyhow!("Configuration must have darkstorm blobstore"))?;
        self.repo_factory
            .redaction_config_blobstore_from_config(blobstore_config)
            .await
    }

    fn redaction_scuba_builder(&self) -> Result<MononokeScubaSampleBuilder> {
        let params = &self.repo_configs.common.censored_scuba_params;
        let mut builder =
            MononokeScubaSampleBuilder::with_opt_table(self.env.fb, params.table.clone());
        if let Some(file) = &params.local_path {
            builder = builder
                .with_log_file(file)
                .context("Failed to open scuba log file")?;
        }

        Ok(builder)
    }
}
