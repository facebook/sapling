/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
#[cfg(fbcode_build)]
use blobstore_factory::ManifoldArgs;
use blobstore_factory::{
    BlobstoreArgs, BlobstoreOptions, CachelibBlobstoreOptions, ChaosOptions, DelayOptions,
    PackOptions, ReadOnlyStorage, ReadOnlyStorageArgs, ThrottleOptions,
};
use cached_config::{ConfigHandle, ConfigStore};
use clap::{App, AppSettings, Args, FromArgMatches, IntoApp};
use cmdlib_caching::{init_cachelib, CachelibArgs, CachelibSettings};
use cmdlib_logging::{
    create_log_level, create_logger, create_observability_context, create_root_log_drain,
    create_scuba_sample_builder, create_warm_bookmark_cache_scuba_sample_builder, LoggingArgs,
    ScubaLoggingArgs,
};
use derived_data_remote::RemoteDerivationArgs;
use environment::MononokeEnvironment;
use fbinit::FacebookInit;
use megarepo_config::{MegarepoConfigsArgs, MononokeMegarepoConfigsOptions};
use mononoke_args::config::ConfigArgs;
use mononoke_args::mysql::MysqlArgs;
use mononoke_args::parse_config_spec_to_path;
use mononoke_args::runtime::RuntimeArgs;
use mononoke_args::tunables::TunablesArgs;
use rendezvous::RendezVousArgs;
use slog::{debug, o, Logger};
use sql_ext::facebook::{MysqlOptions, PoolConfig, ReadConnectionType, SharedConnectionPool};
use tokio::runtime::Runtime;
use tunables;

use crate::app::MononokeApp;
use crate::extension::{ArgExtension, ArgExtensionBox};

pub struct MononokeAppBuilder {
    fb: FacebookInit,
    arg_extensions: Vec<Box<dyn ArgExtensionBox>>,
    cachelib_settings: CachelibSettings,
    readonly_storage: ReadOnlyStorage,
    default_scuba_dataset: Option<String>,
    defaults: HashMap<&'static str, String>,
}

#[derive(Args, Debug)]
pub struct EnvironmentArgs {
    #[clap(flatten, help_heading = "CONFIG OPTIONS")]
    config_args: ConfigArgs,

    #[clap(flatten, help_heading = "RUNTIME OPTIONS")]
    runtime_args: RuntimeArgs,

    #[clap(flatten, help_heading = "LOGGING OPTIONS")]
    logging_args: LoggingArgs,

    #[clap(flatten, help_heading = "SCUBA LOGGING OPTIONS")]
    scuba_logging_args: ScubaLoggingArgs,

    #[clap(flatten, help_heading = "CACHELIB OPTIONS")]
    cachelib_args: CachelibArgs,

    #[clap(flatten, help_heading = "MYSQL OPTIONS")]
    mysql_args: MysqlArgs,

    #[clap(flatten, help_heading = "TUNABLES OPTIONS")]
    tunables_args: TunablesArgs,

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
            arg_extensions: Vec::new(),
            cachelib_settings: CachelibSettings::default(),
            readonly_storage: ReadOnlyStorage(false),
            default_scuba_dataset: None,
            defaults: HashMap::new(),
        }
    }

    pub fn with_default_readonly_storage(mut self, readonly_storage: bool) -> Self {
        self.readonly_storage = ReadOnlyStorage(readonly_storage);
        self
    }

    pub fn with_default_scuba_dataset(mut self, default: impl Into<String>) -> Self {
        self.default_scuba_dataset = Some(default.into());
        self
    }

    pub fn with_default_cachelib_settings(mut self, cachelib_settings: CachelibSettings) -> Self {
        self.cachelib_settings = cachelib_settings;
        self
    }

    pub fn with_arg_extension<Ext>(mut self, ext: Ext) -> Self
    where
        Ext: ArgExtension + 'static,
    {
        self.arg_extensions.push(Box::new(ext));
        self
    }

    pub fn build<AppArgs>(&mut self) -> Result<MononokeApp>
    where
        AppArgs: IntoApp,
    {
        self.build_with_subcommands::<AppArgs>(Vec::new())
    }

    pub fn build_with_subcommands<'sub, AppArgs>(
        &'sub mut self,
        subcommands: Vec<App<'sub>>,
    ) -> Result<MononokeApp>
    where
        AppArgs: IntoApp,
    {
        for defaults in [
            self.readonly_storage.arg_defaults(),
            self.cachelib_settings.arg_defaults(),
        ] {
            for (arg, default) in defaults {
                self.defaults.insert(arg, default);
            }
        }

        for ext in self.arg_extensions.iter() {
            for (arg, default) in ext.arg_defaults() {
                self.defaults.insert(arg, default);
            }
        }

        let mut app = AppArgs::into_app();

        // Save app-generated about so we can restore it.
        let about = app.get_about();
        let long_about = app.get_long_about();

        app = EnvironmentArgs::augment_args_for_update(app);
        for ext in self.arg_extensions.iter() {
            app = ext.augment_args(app);
        }

        // Adding the additional args overrode the about messages.
        // Restore them.
        app = app.about(about).long_about(long_about);

        if !subcommands.is_empty() {
            app = app
                .subcommands(subcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp);
        }

        for (name, default) in self.defaults.iter() {
            app = app.mut_arg(*name, |arg| arg.default_value(default.as_str()));
        }

        let args = app.get_matches();
        let env_args = EnvironmentArgs::from_arg_matches(&args)?;
        let mut env = self.build_environment(env_args)?;

        for ext in self.arg_extensions.iter() {
            ext.process_args(&args, &mut env)?;
        }

        MononokeApp::new(self.fb, args, env)
    }

    fn build_environment(&self, env_args: EnvironmentArgs) -> Result<MononokeEnvironment> {
        let EnvironmentArgs {
            blobstore_args,
            config_args,
            runtime_args,
            logging_args,
            scuba_logging_args,
            cachelib_args,
            #[cfg(fbcode_build)]
            manifold_args,
            megarepo_configs_args,
            mysql_args,
            readonly_storage_args,
            remote_derivation_args,
            rendezvous_args,
            tunables_args,
        } = env_args;

        let log_level = create_log_level(&logging_args);
        #[cfg(fbcode_build)]
        cmdlib_logging::set_glog_log_level(self.fb, log_level)?;
        let root_log_drain = create_root_log_drain(self.fb, &logging_args, log_level)
            .context("Failed to create root log drain")?;

        let config_store = create_config_store(
            self.fb,
            &config_args,
            Logger::root(root_log_drain.clone(), o![]),
        )
        .context("Failed to create config store")?;

        let observability_context =
            create_observability_context(&logging_args, &config_store, log_level)
                .context("Failed to initialize observability context")?;

        let logger = create_logger(
            &logging_args,
            root_log_drain.clone(),
            observability_context.clone(),
        )?;

        let scuba_sample_builder = create_scuba_sample_builder(
            self.fb,
            &scuba_logging_args,
            &observability_context,
            &self.default_scuba_dataset,
        )
        .context("Failed to create scuba sample builder")?;
        let warm_bookmarks_cache_scuba_sample_builder =
            create_warm_bookmark_cache_scuba_sample_builder(self.fb, &scuba_logging_args)
                .context("Failed to create warm bookmark cache scuba sample builder")?;

        let caching = init_cachelib(self.fb, &self.cachelib_settings, &cachelib_args);

        let runtime = create_runtime(&runtime_args)?;

        let mysql_options =
            create_mysql_options(&mysql_args, create_mysql_pool_config(&mysql_args));

        let blobstore_options = create_blobstore_options(
            &blobstore_args,
            &mysql_args,
            #[cfg(fbcode_build)]
            manifold_args,
        )
        .context("Failed to parse blobstore options")?;

        let readonly_storage = ReadOnlyStorage::from_args(&readonly_storage_args);

        let rendezvous_options = rendezvous_args.into();

        let megarepo_configs_options =
            MononokeMegarepoConfigsOptions::from_args(&config_args, &megarepo_configs_args);

        let remote_derivation_options = remote_derivation_args.into();

        init_tunables_worker(&tunables_args, &config_store, logger.clone())?;

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
    config_args: &ConfigArgs,
    logger: Logger,
) -> Result<ConfigStore> {
    const CRYPTO_PROJECT: &str = "SCM";
    const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
    const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

    if let Some(path) = &config_args.local_configerator_path {
        Ok(ConfigStore::file(
            logger,
            path.clone(),
            String::new(),
            CONFIGERATOR_POLL_INTERVAL,
        ))
    } else {
        let crypto_regex_paths = match &config_args.crypto_path_regex {
            Some(paths) => paths.clone(),
            None => vec![
                "scm/mononoke/tunables/.*".to_string(),
                "scm/mononoke/repos/.*".to_string(),
                "scm/mononoke/redaction/.*".to_string(),
            ],
        };
        let crypto_regex = crypto_regex_paths
            .into_iter()
            .map(|path| (path, CRYPTO_PROJECT.to_string()))
            .collect();
        ConfigStore::regex_signed_configerator(
            fb,
            logger,
            crypto_regex,
            CONFIGERATOR_POLL_INTERVAL,
            CONFIGERATOR_REFRESH_TIMEOUT,
        )
    }
}

fn create_runtime(runtime_args: &RuntimeArgs) -> Result<Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    builder.thread_name("tk");
    if let Some(threads) = runtime_args.runtime_threads {
        builder.worker_threads(threads);
    }
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

    let cachelib_blobstore_options =
        CachelibBlobstoreOptions::new_lazy(Some(blobstore_args.blobstore_cachelib_attempt_zstd));

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

    Ok(blobstore_options)
}

fn init_tunables_worker(
    tunables_args: &TunablesArgs,
    config_store: &ConfigStore,
    logger: Logger,
) -> Result<()> {
    if tunables_args.disable_tunables {
        debug!(logger, "Tunables are disabled");
        return Ok(());
    }

    if let Some(tunables_local_path) = &tunables_args.tunables_local_path {
        let value = std::fs::read_to_string(tunables_local_path)
            .with_context(|| format!("failed to open tunables path {}", tunables_local_path))?;
        let config_handle = ConfigHandle::from_json(&value)
            .with_context(|| format!("failed to parse tunables at path {}", tunables_local_path))?;
        return tunables::init_tunables_worker(logger, config_handle);
    }

    let tunables_config = tunables_args.tunables_config_or_default();
    let config_handle =
        config_store.get_config_handle(parse_config_spec_to_path(&tunables_config)?)?;

    tunables::init_tunables_worker(logger, config_handle)
}
