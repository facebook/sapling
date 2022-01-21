/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::{Context, Result};
#[cfg(fbcode_build)]
use blobstore_factory::ManifoldArgs;
use blobstore_factory::{
    BlobstoreArgs, BlobstoreOptions, CachelibBlobstoreOptions, ChaosOptions, DelayOptions,
    PackOptions, ReadOnlyStorage, ReadOnlyStorageArgs, ThrottleOptions,
};
use cached_config::ConfigStore;
use clap::{App, AppSettings, Args, FromArgMatches, IntoApp};
use cmdlib_logging::{create_log_level, create_logger, create_root_log_drain, LoggingArgs};
use derived_data_remote::RemoteDerivationArgs;
use environment::{Caching, MononokeEnvironment};
use fbinit::FacebookInit;
use megarepo_config::{MegarepoConfigsArgs, MononokeMegarepoConfigsOptions};
use mononoke_args::config::ConfigArgs;
use mononoke_args::mysql::MysqlArgs;
use observability::ObservabilityContext;
use rendezvous::RendezVousArgs;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{o, Logger};
use sql_ext::facebook::{MysqlOptions, PoolConfig, ReadConnectionType, SharedConnectionPool};
use tokio::runtime::Runtime;

use crate::app::MononokeApp;

pub struct MononokeAppBuilder {
    fb: FacebookInit,
    readonly_storage_default: ReadOnlyStorage,
}

#[derive(Args, Debug)]
pub struct EnvironmentArgs {
    #[clap(flatten, help_heading = "CONFIG OPTIONS")]
    config_args: ConfigArgs,

    #[clap(flatten, help_heading = "LOGGING OPTIONS")]
    logging_args: LoggingArgs,

    #[clap(flatten, help_heading = "MYSQL OPTIONS")]
    mysql_args: MysqlArgs,

    #[clap(flatten, help_heading = "BLOBSTORE OPTIONS")]
    blobstore_args: BlobstoreArgs,

    #[cfg(fbcode_build)]
    #[clap(flatten, help_heading = "MANIFOLD OPTIONS")]
    manifold_args: ManifoldArgs,

    #[clap(flatten, help_heading = "REMOTE DERIVATION OPTIONS")]
    remote_derivation_args: RemoteDerivationArgs,

    #[clap(flatten, help_heading = "STORAGE OPTIONS")]
    readonly_storage_args: ReadOnlyStorageArgs,

    #[clap(flatten, help_heading = "RENDEZ-VOUS OPTIONS")]
    rendezvous_args: RendezVousArgs,

    #[clap(flatten, help_heading = "MEGAREPO OPTIONS")]
    megarepo_configs_args: MegarepoConfigsArgs,
}

impl MononokeAppBuilder {
    pub fn new(fb: FacebookInit) -> Self {
        MononokeAppBuilder {
            fb,
            readonly_storage_default: ReadOnlyStorage(false),
        }
    }

    pub fn build<AppArgs>(self) -> Result<MononokeApp>
    where
        AppArgs: IntoApp,
    {
        self.build_with_subcommands::<AppArgs>(Vec::new())
    }

    pub fn build_with_subcommands<'help, AppArgs>(
        self,
        subcommands: Vec<App<'help>>,
    ) -> Result<MononokeApp>
    where
        AppArgs: IntoApp,
    {
        let mut app = AppArgs::into_app();

        // Save app-generated about so we can restore it.
        let about = app.get_about();
        let long_about = app.get_long_about();

        app = EnvironmentArgs::augment_args_for_update(app);

        // Adding the additional args overrode the about messages.
        // Restore them.
        app = app.about(about).long_about(long_about);

        if !subcommands.is_empty() {
            app = app
                .subcommands(subcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp);
        }

        let args = app.get_matches();
        let env_args = EnvironmentArgs::from_arg_matches(&args)?;
        let env = self.build_environment(env_args)?;

        // TODO: create TunablesArgs and init tunables
        // TODO: maybe_enable_mcrouter

        MononokeApp::new(self.fb, args, env)
    }

    fn build_environment(&self, env_args: EnvironmentArgs) -> Result<MononokeEnvironment> {
        let EnvironmentArgs {
            blobstore_args,
            config_args,
            logging_args,
            #[cfg(fbcode_build)]
            manifold_args,
            megarepo_configs_args,
            mysql_args,
            readonly_storage_args,
            remote_derivation_args,
            rendezvous_args,
        } = env_args;

        let log_level = create_log_level(&logging_args);
        let root_log_drain = create_root_log_drain(self.fb, &logging_args, log_level)
            .context("Failed to create root log drain")?;

        let config_store = create_config_store(
            self.fb,
            &config_args,
            Logger::root(root_log_drain.clone(), o![]),
        )
        .context("Failed to create config store")?;

        // TODO: create ObvservabilityArgs
        let observability_context = create_observability_context(&config_store, log_level)
            .context("Failed to initialize observability context")?;

        let logger = create_logger(
            &logging_args,
            root_log_drain.clone(),
            observability_context.clone(),
        )?;

        // TODO: create ScubaArgs, plumb through other options
        let scuba_sample_builder = create_scuba_sample_builder(self.fb)
            .context("Failed to create scuba sample builder")?;
        let warm_bookmarks_cache_scuba_sample_builder =
            create_warm_bookmarks_cache_scuba_sample_builder(self.fb)
                .context("Failed to create warm bookmark cache scuba sample builder")?;

        // TODO: create CacheArgs and plumb through CachelibSettings
        let caching = init_cachelib();

        // TODO: create RuntimeArgs
        let runtime = create_runtime()?;

        let mysql_options =
            create_mysql_options(&mysql_args, create_mysql_pool_config(&mysql_args));

        let blobstore_options = create_blobstore_options(
            &blobstore_args,
            &mysql_args,
            #[cfg(fbcode_build)]
            manifold_args,
        )
        .context("Failed to parse blobstore options")?;

        let readonly_storage = ReadOnlyStorage(
            readonly_storage_args
                .with_readonly_storage
                .unwrap_or(self.readonly_storage_default.0),
        );

        let rendezvous_options = rendezvous_args.into();

        let megarepo_configs_options =
            MononokeMegarepoConfigsOptions::from_args(&config_args, &megarepo_configs_args);

        let remote_derivation_options = remote_derivation_args.into();

        Ok(MononokeEnvironment {
            fb: self.fb,
            logger,
            scuba_sample_builder,
            warm_bookmarks_cache_scuba_sample_builder,
            config_store,
            caching,
            observability_context,
            runtime,
            mysql_options,
            blobstore_options,
            readonly_storage,
            rendezvous_options,
            megarepo_configs_options,
            remote_derivation_options,
        })
    }
}

fn create_config_store(
    fb: FacebookInit,
    _config_args: &ConfigArgs,
    logger: Logger,
) -> Result<ConfigStore> {
    const CRYPTO_PROJECT: &str = "SCM";
    const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
    const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

    // TODO: use local_configerator_path from config args
    // TODO: add crypto_path_regex to config args
    let crypto_regex_paths = vec![
        "scm/mononoke/tunables/.*",
        "scm/mononoke/repos/.*",
        "scm/mononoke/redaction/.*",
    ];
    let crypto_regex = crypto_regex_paths
        .into_iter()
        .map(|path| (path.to_string(), CRYPTO_PROJECT.to_string()))
        .collect();
    ConfigStore::regex_signed_configerator(
        fb,
        logger,
        crypto_regex,
        CONFIGERATOR_POLL_INTERVAL,
        CONFIGERATOR_REFRESH_TIMEOUT,
    )
}

fn create_observability_context(
    // observability_args: &ObservabilityArgs,
    _config_store: &ConfigStore,
    log_level: slog::Level,
) -> Result<ObservabilityContext> {
    Ok(ObservabilityContext::new_static(log_level))
}

fn create_scuba_sample_builder(_fb: FacebookInit) -> Result<MononokeScubaSampleBuilder> {
    Ok(MononokeScubaSampleBuilder::with_discard())
}

fn create_warm_bookmarks_cache_scuba_sample_builder(
    _fb: FacebookInit,
) -> Result<MononokeScubaSampleBuilder> {
    Ok(MononokeScubaSampleBuilder::with_discard())
}

fn init_cachelib() -> Caching {
    Caching::Disabled
}

fn create_runtime() -> Result<Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    builder.thread_name("tk");
    // builder.worker_threads(worker_threads);
    let runtime = builder.build()?;
    Ok(runtime)
}

fn create_mysql_options(mysql_args: &MysqlArgs, pool_config: PoolConfig) -> MysqlOptions {
    let pool = SharedConnectionPool::new();
    let read_connection_type = if mysql_args.mysql_master_only {
        ReadConnectionType::Master
    } else {
        ReadConnectionType::ReplicaOnly
    };
    MysqlOptions {
        pool,
        pool_config,
        read_connection_type,
    }
}

fn create_mysql_pool_config(mysql_args: &MysqlArgs) -> PoolConfig {
    PoolConfig::new(
        mysql_args.mysql_pool_limit,
        mysql_args.mysql_pool_threads_num,
        mysql_args.mysql_pool_per_key_limit,
        mysql_args.mysql_pool_age_timeout,
        mysql_args.mysql_pool_idle_timeout,
        mysql_args.mysql_conn_open_timeout,
        Duration::from_millis(mysql_args.mysql_max_query_time),
    )
}

fn create_mysql_sqlblob_pool_config(mysql_args: &MysqlArgs) -> PoolConfig {
    PoolConfig::new(
        mysql_args.mysql_sqlblob_pool_limit,
        mysql_args.mysql_sqlblob_pool_threads_num,
        mysql_args.mysql_sqlblob_pool_per_key_limit,
        mysql_args.mysql_sqlblob_pool_age_timeout,
        mysql_args.mysql_sqlblob_pool_idle_timeout,
        mysql_args.mysql_conn_open_timeout,
        Duration::from_millis(mysql_args.mysql_max_query_time),
    )
}

fn create_blobstore_options(
    blobstore_args: &BlobstoreArgs,
    mysql_args: &MysqlArgs,
    #[cfg(fbcode_build)] manifold_args: ManifoldArgs,
) -> Result<BlobstoreOptions> {
    let chaos_options = ChaosOptions::new(
        blobstore_args.blobstore_read_chaos_rate,
        blobstore_args.blobstore_write_chaos_rate,
    );

    let delay_options = DelayOptions {
        get_dist: blobstore_args.get_delay_distribution()?,
        put_dist: blobstore_args.put_delay_distribution()?,
    };

    let throttle_options = ThrottleOptions {
        read_qps: blobstore_args.blobstore_read_qps,
        write_qps: blobstore_args.blobstore_write_qps,
        read_bytes: blobstore_args.blobstore_read_bytes_s,
        write_bytes: blobstore_args.blobstore_write_bytes_s,
        read_burst_bytes: blobstore_args.blobstore_read_burst_bytes_s,
        write_burst_bytes: blobstore_args.blobstore_write_burst_bytes_s,
        bytes_min_count: blobstore_args.blobstore_bytes_min_throttle,
    };

    let pack_options = PackOptions::new(blobstore_args.put_format_override()?);

    let cachelib_blobstore_options = CachelibBlobstoreOptions::new_lazy(Some(
        blobstore_args
            .blobstore_cachelib_attempt_zstd
            .unwrap_or(false),
    ));

    let blobstore_put_behaviour = blobstore_args.blobstore_put_behaviour;

    let mysql_sqlblob_options =
        create_mysql_options(mysql_args, create_mysql_sqlblob_pool_config(mysql_args));

    let blobstore_options = BlobstoreOptions::new(
        chaos_options,
        delay_options,
        throttle_options,
        #[cfg(fbcode_build)]
        manifold_args.into(),
        pack_options,
        cachelib_blobstore_options,
        blobstore_put_behaviour,
        mysql_sqlblob_options,
    );

    // TODO: add scrub args if requested

    Ok(blobstore_options)
}
