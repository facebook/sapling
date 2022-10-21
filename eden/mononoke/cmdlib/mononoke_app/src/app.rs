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
use std::time::Instant;

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
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
use futures_util::try_join;
use itertools::Itertools;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use mononoke_configs::ConfigUpdateReceiver;
use mononoke_configs::MononokeConfigs;
use mononoke_repos::MononokeRepos;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use redactedblobstore::RedactedBlobstore;
use redactedblobstore::RedactedBlobstoreConfig;
use redactedblobstore::RedactionConfigBlobstore;
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use scuba_ext::MononokeScubaSampleBuilder;
use services::Fb303Service;
use slog::error;
use slog::info;
use slog::o;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use stats::prelude::*;
#[cfg(not(test))]
use stats::schedule_stats_aggregation_preview;
use tokio::runtime::Handle;

use crate::args::ConfigArgs;
use crate::args::ConfigMode;
use crate::args::MultiRepoArgs;
use crate::args::RepoArg;
use crate::args::RepoArgs;
use crate::args::RepoBlobstoreArgs;
use crate::args::SourceAndTargetRepoArg;
use crate::args::SourceAndTargetRepoArgs;
use crate::extension::AppExtension;
use crate::extension::AppExtensionArgsBox;
use crate::extension::BoxedAppExtensionArgs;
use crate::fb303::Fb303AppExtension;

define_stats! {
    prefix = "mononoke.app";
    initialization_time_millisecs: dynamic_timeseries(
        "initialization_time_millisecs.{}",
        (reponame: String);
        Average, Sum, Count
    ),
}

/// Struct responsible for receiving updated configurations from MononokeConfigs
/// and refreshing repos (and related entities) based on the update.
pub struct MononokeConfigUpdateReceiver<Repo> {
    repos: Arc<MononokeRepos<Repo>>,
    repo_factory: RepoFactory,
    logger: Logger,
}

impl<Repo> MononokeConfigUpdateReceiver<Repo> {
    fn new(repos: Arc<MononokeRepos<Repo>>, app: &MononokeApp) -> Self {
        Self {
            repos,
            repo_factory: app.repo_factory(),
            logger: app.logger().clone(),
        }
    }
}

#[async_trait]
impl<Repo> ConfigUpdateReceiver for MononokeConfigUpdateReceiver<Repo>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>> + Send + Sync,
{
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        _: Arc<StorageConfigs>,
    ) -> Result<()> {
        // We need to filter out the name of repos that are present in MononokeRepos (i.e.
        // currently served by the server) but not in RepoConfigs. This situation can happen
        // when the name of the repo changes (e.g. whatsapp/server.mirror renamed to whatsapp/server)
        // or when a repo is added or removed. In such a case, reloading of the repo with the old name
        // would not be possible based on the new configs.
        let repos_input = stream::iter(self.repos.iter_names().filter_map(|repo_name| {
            repo_configs
                .repos
                .get(&repo_name)
                .cloned()
                .map(|repo_config| (repo_name, repo_config))
        }))
        .map(|(repo_name, repo_config)| {
            let repo_factory = self.repo_factory.clone();
            let name = repo_name.clone();
            let logger = self.logger.clone();
            let common_config = repo_configs.common.clone();
            async move {
                let repo_id = repo_config.repoid.id();
                info!(logger, "Reloading repo: {}", &repo_name);
                let repo = repo_factory
                    .build(name, repo_config, common_config)
                    .await
                    .with_context(|| format!("Failed to reload repo '{}'", &repo_name))?;
                info!(logger, "Reloaded repo: {}", &repo_name);

                Ok::<_, Error>((repo_id, repo_name, repo))
            }
        })
        // Repo construction can be heavy, 30 at a time is sufficient.
        .buffered(30)
        .collect::<Vec<_>>();
        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_input = tokio::task::unconstrained(repos_input)
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        self.repos.populate(repos_input);
        Ok(())
    }
}

pub struct MononokeApp {
    pub fb: FacebookInit,
    config_mode: ConfigMode,
    args: ArgMatches,
    env: Arc<MononokeEnvironment>,
    extension_args: HashMap<TypeId, Box<dyn BoxedAppExtensionArgs>>,
    configs: MononokeConfigs,
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
        let configs = MononokeConfigs::new(
            config_path,
            config_store,
            env.runtime.handle().clone(),
            env.logger.clone(),
        )?;

        let repo_factory = RepoFactory::new(env.clone());

        Ok(MononokeApp {
            fb,
            config_mode,
            args,
            env,
            extension_args,
            configs,
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
    ///
    /// This command doesn't provide anything mnore than executiing the provided future
    /// it won't handle things like fb303 data collection, it's not a drop-in replacement
    /// for cmdlib::block_execute (run_with_fb303_monitoring will do better here).
    pub fn run_basic<F, Fut>(self, main: F) -> Result<()>
    where
        F: Fn(MononokeApp) -> Fut,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let env = self.env.clone();
        env.runtime
            .block_on(async move { tokio::spawn(main(self)).await? })
    }

    /// Execute a future on this app's runtime and start fb303 monitoring
    /// service for the app.
    pub fn run_with_fb303_monitoring<F, Fut, S: Fb303Service + Sync + Send + 'static>(
        self,
        main: F,
        app_name: &str,
        service: S,
    ) -> Result<()>
    where
        F: Fn(MononokeApp) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let env = self.env.clone();
        let logger = self.logger().clone();
        let fb303_args = self.extension_args::<Fb303AppExtension>()?;
        fb303_args.start_fb303_server(self.fb, app_name, self.logger(), service)?;
        let result = env.runtime.block_on(async move {
            #[cfg(not(test))]
            {
                let stats_agg = schedule_stats_aggregation_preview()
                    .map_err(|_| Error::msg("Failed to create stats aggregation worker"))?;
                // Note: this returns a JoinHandle, which we drop, thus detaching the task
                // It thus does not count towards shutdown_on_idle below
                tokio::task::spawn(stats_agg);
            }

            main(self).await
        });

        // Log error in glog format (main will log, but not with glog)
        result.map_err(move |e| {
            error!(&logger, "Execution error: {:?}", e);
            // Shorten the error that main will print, given that already printed in glog form
            format_err!("Execution failed")
        })
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
    pub fn repo_configs(&self) -> Arc<RepoConfigs> {
        self.configs.repo_configs()
    }

    /// The storage configs for this app.
    pub fn storage_configs(&self) -> Arc<StorageConfigs> {
        self.configs.storage_configs()
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

    /// Create a basic CoreContext without scuba logging.  Good choice for
    /// simple CLI tools like admin.
    ///
    /// Warning: returned context doesn't provide any scuba logging!
    pub fn new_basic_context(&self) -> CoreContext {
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

    pub fn repo_config_by_name(&self, repo_name: &str) -> Result<RepoConfig> {
        self.repo_configs()
            .repos
            .get(repo_name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))
    }

    /// Get repo config based on user-provided arguments.
    pub fn repo_config(&self, repo_arg: RepoArg) -> Result<(String, RepoConfig)> {
        match repo_arg {
            RepoArg::Id(repo_id) => {
                let repo_configs = self.repo_configs();
                let (repo_name, repo_config) = repo_configs
                    .get_repo_config(repo_id)
                    .ok_or_else(|| anyhow!("unknown repoid: {:?}", repo_id))?;
                Ok((repo_name.clone(), repo_config.clone()))
            }
            RepoArg::Name(repo_name) => {
                let repo_config = self.repo_config_by_name(repo_name)?;
                Ok((repo_name.to_string(), repo_config))
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

    /// Get source and target repo configs based on user-provided arguments.
    pub fn source_and_target_repo_config(
        &self,
        repo_arg: SourceAndTargetRepoArg,
    ) -> Result<((String, RepoConfig), (String, RepoConfig))> {
        Ok((
            self.repo_config(repo_arg.source_repo)?,
            self.repo_config(repo_arg.target_repo)?,
        ))
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
        let common_config = self.repo_configs().common.clone();
        let repos: Vec<_> = stream::iter(repos)
            .map(|(repo_name, repo_config)| {
                let repo_factory = self.repo_factory.clone();
                let common_config = common_config.clone();
                async move {
                    repo_factory
                        .build(repo_name, repo_config, common_config)
                        .await
                }
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
        let common_config = self.repo_configs().common.clone();
        let repo = self
            .repo_factory
            .build(repo_name, repo_config, common_config)
            .await?;
        Ok(repo)
    }

    async fn populate_repos<Repo, Names>(
        &self,
        mononoke_repos: &MononokeRepos<Repo>,
        repo_names: Names,
    ) -> Result<()>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repos_input = stream::iter(repo_names.into_iter().unique())
            .map(|repo_name| {
                let repo_factory = self.repo_factory.clone();
                let name = repo_name.clone();
                async move {
                    let start = Instant::now();
                    let logger = self.logger();
                    let repo_config = self.repo_config_by_name(&repo_name)?;
                    let common_config = self.repo_configs().common.clone();
                    let repo_id = repo_config.repoid.id();
                    info!(logger, "Initializing repo: {}", &repo_name);
                    let repo = repo_factory
                        .build(name, repo_config, common_config)
                        .await
                        .with_context(|| format!("Failed to initialize repo '{}'", &repo_name))?;
                    info!(logger, "Initialized repo: {}", &repo_name);
                    STATS::initialization_time_millisecs.add_value(
                        start.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
                        (repo_name.to_string(),),
                    );
                    Ok::<_, Error>((repo_id, repo_name, repo))
                }
            })
            // Repo construction can be heavy, 30 at a time is sufficient.
            .buffered(30)
            .collect::<Vec<_>>();
        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_input = tokio::task::unconstrained(repos_input)
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        mononoke_repos.populate(repos_input);
        Ok(())
    }

    /// Method responsible for constructing repos corresponding to the input
    /// repo-names and populating MononokeRepos with the result. This method
    /// should be used if dynamically reloadable repos (with configs) are
    /// needed. For fixed set of repos, use open_repo or open_repos.
    pub async fn open_mononoke_repos<Repo, Names>(
        &self,
        repo_names: Names,
    ) -> Result<Arc<MononokeRepos<Repo>>>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let mononoke_repos = MononokeRepos::new();
        self.populate_repos(&mononoke_repos, repo_names).await?;
        let mononoke_repos = Arc::new(mononoke_repos);
        let update_receiver = MononokeConfigUpdateReceiver::new(mononoke_repos.clone(), self);
        self.configs
            .register_for_update(Arc::new(update_receiver) as Arc<dyn ConfigUpdateReceiver>);
        Ok(mononoke_repos)
    }

    /// Method responsible for constructing and adding a new repo to the
    /// passed-in MononokeRepos instance.
    pub async fn add_repo<Repo>(
        &self,
        repos: &Arc<MononokeRepos<Repo>>,
        repo_name: &str,
    ) -> Result<()>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_config = self.repo_config_by_name(repo_name)?;
        let repo_id = repo_config.repoid.id();
        let common_config = &self.repo_configs().common;
        let repo = self
            .repo_factory
            .build(repo_name.to_string(), repo_config, common_config.clone())
            .await?;
        repos.add(repo_name, repo_id, repo);
        Ok(())
    }

    /// Method responsible for removing an existing repo based on the input
    /// repo-name from the passed-in MononokeRepos instance.
    pub fn remove_repo<Repo>(&self, repos: &Arc<MononokeRepos<Repo>>, repo_name: &str) {
        repos.remove(repo_name);
    }

    /// Method responsible for reloading the current set of loaded repos within
    /// MononokeApp. The reload will involve reconstruction of the repos using
    /// the current version of the RepoConfig. The old repos will be dropped
    /// once all references to it cease to exist.
    pub async fn reload_repos<Repo>(&self, repos: &Arc<MononokeRepos<Repo>>) -> Result<()>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_names = repos.iter_names();
        self.populate_repos(repos, repo_names).await?;
        Ok(())
    }

    /// Open a source and target repos based on user-provided arguments.
    pub async fn open_source_and_target_repos<Repo>(
        &self,
        repo_args: &SourceAndTargetRepoArgs,
    ) -> Result<(Repo, Repo)>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repos = repo_args.source_and_target_id_or_name()?;
        let source_repo_arg = repos.source_repo;
        let target_repo_arg = repos.target_repo;
        let (source_repo_name, source_repo_config) = self.repo_config(source_repo_arg)?;
        let (target_repo_name, target_repo_config) = self.repo_config(target_repo_arg)?;
        let common_config = self.repo_configs().common.clone();
        let source_repo_fut =
            self.repo_factory
                .build(source_repo_name, source_repo_config, common_config.clone());
        let target_repo_fut =
            self.repo_factory
                .build(target_repo_name, target_repo_config, common_config);

        let (source_repo, target_repo) = try_join!(source_repo_fut, target_repo_fut)?;
        Ok((source_repo, target_repo))
    }

    /// Open just the blobstore based on user-provided arguments.
    pub async fn open_blobstore(
        &self,
        repo_blobstore_args: &RepoBlobstoreArgs,
    ) -> Result<Arc<dyn Blobstore>> {
        let repo_configs = self.repo_configs();
        let storage_configs = self.storage_configs();
        let (mut repo_id, redaction, storage_config) =
            if let Some(repo_id) = repo_blobstore_args.repo_id {
                let repo_id = RepositoryId::new(repo_id);
                let (_repo_name, repo_config) = repo_configs
                    .get_repo_config(repo_id)
                    .ok_or_else(|| anyhow!("unknown repoid: {:?}", repo_id))?;
                (
                    Some(repo_id),
                    repo_config.redaction,
                    repo_config.storage_config.clone(),
                )
            } else if let Some(repo_name) = &repo_blobstore_args.repo_name {
                let repo_config = repo_configs
                    .repos
                    .get(repo_name)
                    .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))?;
                (
                    Some(repo_config.repoid),
                    repo_config.redaction,
                    repo_config.storage_config.clone(),
                )
            } else if let Some(storage_name) = &repo_blobstore_args.storage_name {
                let storage_config = storage_configs
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
                .redacted_blobs(
                    self.new_basic_context(),
                    &storage_config.metadata,
                    &Arc::new(self.repo_configs().common.clone()),
                )
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
                &self.repo_configs().common.redaction_config.blobstore,
            )
            .await
    }

    pub async fn redaction_config_blobstore_for_darkstorm(
        &self,
    ) -> Result<Arc<RedactionConfigBlobstore>> {
        let blobstore_config = self
            .repo_configs()
            .common
            .redaction_config
            .darkstorm_blobstore
            .clone()
            .ok_or_else(|| anyhow!("Configuration must have darkstorm blobstore"))?;
        self.repo_factory
            .redaction_config_blobstore_from_config(&blobstore_config)
            .await
    }

    fn redaction_scuba_builder(&self) -> Result<MononokeScubaSampleBuilder> {
        let params = &self.repo_configs().common.censored_scuba_params;
        let mut builder =
            MononokeScubaSampleBuilder::with_opt_table(self.env.fb, params.table.clone())?;
        if let Some(file) = &params.local_path {
            builder = builder
                .with_log_file(file)
                .context("Failed to open scuba log file")?;
        }

        Ok(builder)
    }
}
