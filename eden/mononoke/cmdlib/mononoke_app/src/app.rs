/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::TypeId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::bail;
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
use cmdlib_running::run_until_terminated;
use context::CoreContext;
use environment::MononokeEnvironment;
use facet::AsyncBuildable;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_util::try_join;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use mononoke_configs::MononokeConfigs;
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
use tokio::sync::oneshot;

use crate::args::AsRepoArg;
use crate::args::ConfigArgs;
use crate::args::ConfigMode;
use crate::args::MultiRepoArgs;
use crate::args::RepoArg;
use crate::args::RepoBlobstoreArgs;
use crate::args::SourceAndTargetRepoArgs;
use crate::extension::AppExtension;
use crate::extension::AppExtensionArgsBox;
use crate::extension::BoxedAppExtensionArgs;
use crate::fb303::Fb303AppExtension;
use crate::repos_manager::MononokeReposManager;

define_stats! {
    prefix = "mononoke.app";
    completion_duration_secs: timeseries(Average, Sum, Count),
}

pub struct MononokeApp {
    pub fb: FacebookInit,
    config_mode: ConfigMode,
    args: ArgMatches,
    env: Arc<MononokeEnvironment>,
    extension_args: HashMap<TypeId, Box<dyn BoxedAppExtensionArgs>>,
    configs: Arc<MononokeConfigs>,
    repo_factory: Arc<RepoFactory>,
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
        let configs = Arc::new(MononokeConfigs::new(
            config_path,
            config_store,
            env.runtime.handle().clone(),
            env.logger.clone(),
        )?);

        let repo_factory = Arc::new(RepoFactory::new(env.clone()));

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

    /// Start the FB303 monitoring server for the provided service.
    pub fn start_monitoring<Service>(&self, app_name: &str, service: Service) -> Result<()>
    where
        Service: Fb303Service + Sync + Send + 'static,
    {
        let fb303_args = self.extension_args::<Fb303AppExtension>()?;
        fb303_args.start_fb303_server(self.fb, app_name, self.logger(), service)?;
        Ok(())
    }

    /// Start the background stats aggregation thread.
    pub fn start_stats_aggregation(&self) -> Result<()> {
        #[cfg(not(test))]
        {
            self.env.runtime.block_on(async move {
                let stats_aggregation = schedule_stats_aggregation_preview()
                    .map_err(|_| anyhow!("Failed to create stats aggregation worker"))?;
                tokio::task::spawn(stats_aggregation);
                anyhow::Ok(())
            })?;
        }
        Ok(())
    }

    /// Execute a future on this app's runtime.
    ///
    /// If you are looking for a replacement for `cmdlib::helpers::block_execute`, prefer
    /// `run_with_monitoring_and_logging`.
    pub fn run_basic<F, Fut>(self, main: F) -> Result<()>
    where
        F: Fn(MononokeApp) -> Fut,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let env = self.env.clone();
        env.runtime
            .block_on(async move { tokio::spawn(main(self)).await? })
    }

    /// Execute a future on this app's runtime.
    ///
    /// This future will run with monitoring enabled, and errors will be logged to glog.
    pub fn run_with_monitoring_and_logging<F, Fut, Service>(
        self,
        main: F,
        app_name: &str,
        service: Service,
    ) -> Result<()>
    where
        F: Fn(MononokeApp) -> Fut,
        Fut: Future<Output = Result<()>>,
        Service: Fb303Service + Sync + Send + 'static,
    {
        self.start_monitoring(app_name, service)?;
        self.start_stats_aggregation()?;

        let env = self.env.clone();
        let logger = self.logger().clone();
        let result = env.runtime.block_on(main(self));

        if let Err(e) = result {
            // Log error in glog format
            error!(&logger, "Execution error: {:?}", e);

            // Replace the error with a simple error so it isn't logged twice.
            return Err(anyhow!("Execution failed"));
        }

        Ok(())
    }

    /// Run a server future, and wait until a termination signal is received.
    ///
    /// When the termination signal is received, the `quiesce` callback is
    /// called.  This should perform any steps required to quiesce the server,
    /// for example by removing this instance from routing configuration, or
    /// asking the load balancer to stop sending requests to this instance.
    /// Requests that do arrive should still be accepted.
    ///
    /// After the `shutdown_grace_period`, the `shutdown` future is awaited.
    /// This should do any additional work to stop accepting connections and wait
    /// until all outstanding requests have been handled. The `server` future
    /// continues to run while `shutdown` is being awaited.
    ///
    /// Once both `shutdown` and `server` have completed, the process
    /// exits. If `shutdown_timeout` is exceeded, the server future is canceled
    /// and an error is returned.
    pub fn run_until_terminated<ServerFn, ServerFut, QuiesceFn, ShutdownFut>(
        self,
        server: ServerFn,
        quiesce: QuiesceFn,
        shutdown_grace_period: Duration,
        shutdown: ShutdownFut,
        shutdown_timeout: Duration,
    ) -> Result<()>
    where
        ServerFn: FnOnce(MononokeApp) -> ServerFut + Send + 'static,
        ServerFut: Future<Output = Result<()>> + Send + 'static,
        QuiesceFn: FnOnce(),
        ShutdownFut: Future<Output = ()>,
    {
        let logger = self.logger().clone();
        // We must ensure the runtime (in the environment) outlives the
        // execution of the server future on the runtime.  If we drop the
        // runtime from within a future that is executing on the runtime, then
        // the runtime will panic. Keep a copy of the environment in this
        // function to ensure the runtime is kept alive.
        // TODO(mbthomas): decouple runtime from environment so this isn't necessary
        let env = self.env.clone();
        let server = async move { server(self).await };
        env.runtime.block_on(run_until_terminated(
            server,
            &logger,
            quiesce,
            shutdown_grace_period,
            shutdown,
            shutdown_timeout,
        ))
    }

    /// Wait until a termination signal is received.
    ///
    /// This method does not have a server future, and so is useful when all
    /// serving listeners are running on another executor (e.g. a C++
    /// executor for a thrift service).
    ///
    /// When the termination signal is received, the same quiesce-shutdown
    /// procedure as for `run_until_terminated` is followed.
    pub fn wait_until_terminated<QuiesceFn, ShutdownFut>(
        self,
        quiesce: QuiesceFn,
        shutdown_grace_period: Duration,
        shutdown: ShutdownFut,
        shutdown_timeout: Duration,
    ) -> Result<()>
    where
        QuiesceFn: FnOnce(),
        ShutdownFut: Future<Output = ()>,
    {
        let (exit_tx, exit_rx) = oneshot::channel();
        let server = move |_app| async move {
            exit_rx.await?;
            Ok(())
        };

        self.run_until_terminated(
            server,
            || {
                let _ = exit_tx.send(());
                quiesce();
            },
            shutdown_grace_period,
            shutdown,
            shutdown_timeout,
        )
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
    pub fn repo_factory(&self) -> &Arc<RepoFactory> {
        &self.repo_factory
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
    pub fn repo_config(&self, repo_arg: &RepoArg) -> Result<(String, RepoConfig)> {
        match repo_arg {
            RepoArg::Id(repo_id) => {
                let repo_configs = self.repo_configs();
                let (repo_name, repo_config) = repo_configs
                    .get_repo_config(*repo_id)
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
            let (name, repo_conf) = self.repo_config(&repo)?;
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
            repos.push(self.repo_config(&arg)?);
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
    pub async fn open_repo<Repo>(&self, repo_args: &impl AsRepoArg) -> Result<Repo>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_arg = repo_args.as_repo_arg();
        let (repo_name, repo_config) = self.repo_config(repo_arg)?;
        let common_config = self.repo_configs().common.clone();
        let repo = self
            .repo_factory
            .build(repo_name, repo_config, common_config)
            .await?;
        Ok(repo)
    }

    /// Open an existing repo object
    /// Make sure that the opened repo has redaction DISABLED
    pub async fn open_repo_unredacted<Repo>(&self, repo_args: &impl AsRepoArg) -> Result<Repo>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_arg = repo_args.as_repo_arg();
        let (repo_name, mut repo_config) = self.repo_config(repo_arg)?;
        let common_config = self.repo_configs().common.clone();
        repo_config.redaction = Redaction::Disabled;
        let repo = self
            .repo_factory
            .build(repo_name, repo_config, common_config)
            .await?;
        Ok(repo)
    }

    /// Create a new repo object -- for local instances, expect its contents to be empty.
    /// Makes sure that the opened repo has redaction DISABLED
    pub async fn create_repo_unredacted<Repo>(&self, repo_arg: &impl AsRepoArg) -> Result<Repo>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let (repo_name, mut repo_config) = self.repo_config(repo_arg.as_repo_arg())?;
        let common_config = self.repo_configs().common.clone();

        match &repo_config.storage_config.blobstore {
            BlobConfig::Files { path } | BlobConfig::Sqlite { path } => {
                setup_repo_dir(path, CreateStorage::ExistingOrCreate)?;
            }
            _ => {}
        }
        repo_config.redaction = Redaction::Disabled;
        let repo = self
            .repo_factory
            .build(repo_name, repo_config, common_config)
            .await?;
        Ok(repo)
    }

    /// Open a source and target repos based on user-provided arguments.
    pub async fn open_source_and_target_repos<Repo>(
        &self,
        repo_args: &SourceAndTargetRepoArgs,
    ) -> Result<(Repo, Repo)>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let (source_repo_name, source_repo_config) =
            self.repo_config(repo_args.source_repo.as_repo_arg())?;
        let (target_repo_name, target_repo_config) =
            self.repo_config(repo_args.target_repo.as_repo_arg())?;
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

    /// Create a manager for all configured shallow-sharded repos, excluding
    /// those filtered by `repo_filter_from` in `MononokeEnvironment`.
    pub async fn open_managed_repos<Repo>(&self) -> Result<MononokeReposManager<Repo>>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let repo_filter = self.environment().filter_repos.clone();
        let repo_names =
            self.repo_configs()
                .repos
                .clone()
                .into_iter()
                .filter_map(|(name, config)| {
                    let is_matching_filter =
                        repo_filter.as_ref().map_or(true, |filter| filter(&name));
                    // Initialize repos that are enabled and not deep-sharded (i.e. need to exist
                    // at service startup)
                    if config.enabled && !config.deep_sharded && is_matching_filter {
                        Some(name)
                    } else {
                        None
                    }
                });
        self.open_named_managed_repos(repo_names).await
    }

    /// Create a manager for a set of named managed repos.  These repos must
    /// be configured in the config.
    pub async fn open_named_managed_repos<Repo, Names>(
        &self,
        repo_names: Names,
    ) -> Result<MononokeReposManager<Repo>>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let logger = self.logger().clone();
        let start = Instant::now();
        let repos_mgr = MononokeReposManager::new(
            self.configs.clone(),
            self.repo_factory().clone(),
            self.logger().clone(),
            repo_names,
        )
        .await?;
        info!(
            &logger,
            "All repos initialized. It took: {} seconds",
            start.elapsed().as_secs()
        );
        STATS::completion_duration_secs
            .add_value(start.elapsed().as_secs().try_into().unwrap_or(i64::MAX));
        Ok(repos_mgr)
    }

    /// Create a manager for a single repo, specified by repo arguments.
    pub async fn open_managed_repo_arg<Repo>(
        &self,
        repo_arg: &impl AsRepoArg,
    ) -> Result<MononokeReposManager<Repo>>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let (repo_name, _) = self.repo_config(repo_arg.as_repo_arg())?;
        self.open_named_managed_repos(Some(repo_name)).await
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
                BlobConfig::Multiplexed { blobstores, .. }
                | BlobConfig::MultiplexedWal { blobstores, .. } => {
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CreateStorage {
    ExistingOnly,
    ExistingOrCreate,
}

pub fn setup_repo_dir<P: AsRef<Path>>(data_dir: P, create: CreateStorage) -> Result<()> {
    let data_dir = data_dir.as_ref();

    if !data_dir.is_dir() {
        bail!("{:?} does not exist or is not a directory", data_dir);
    }

    // Validate directory layout
    #[allow(clippy::single_element_loop)]
    for subdir in &["blobs"] {
        let subdir = data_dir.join(subdir);

        if subdir.exists() && !subdir.is_dir() {
            bail!("{:?} already exists and is not a directory", subdir);
        }

        if !subdir.exists() {
            if CreateStorage::ExistingOnly == create {
                bail!("{:?} not found in ExistingOnly mode", subdir,);
            }
            fs::create_dir(&subdir)
                .with_context(|| format!("failed to create subdirectory {:?}", subdir))?;
        }
    }
    Ok(())
}
