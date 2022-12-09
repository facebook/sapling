/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::panic::RefUnwindSafe;
use std::panic::UnwindSafe;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::CachelibBlobstoreOptions;
use blobstore_factory::ChaosOptions;
use blobstore_factory::DelayOptions;
use blobstore_factory::PackOptions;
use blobstore_factory::PutBehaviour;
use blobstore_factory::ScrubAction;
use blobstore_factory::SrubWriteOnly;
use blobstore_factory::ThrottleOptions;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use clap_old::ArgMatches;
use clap_old::OsValues;
use clap_old::Values;
use derived_data_remote::RemoteDerivationOptions;
use environment::Caching;
use environment::MononokeEnvironment;
use fbinit::FacebookInit;
use maybe_owned::MaybeOwned;
use megarepo_config::MononokeMegarepoConfigsOptions;
use metaconfig_types::PackFormat;
use mononoke_app::args::parse_config_spec_to_path;
use observability::DynamicLevelDrain;
use observability::ObservabilityContext;
use panichandler::Fate;
use permission_checker::AclProvider;
use permission_checker::DefaultAclProvider;
use permission_checker::InternalAclProvider;
use rendezvous::RendezVousOptions;
use repo_factory::ReadOnlyStorage;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::o;
use slog::Level;
use slog::Logger;
use slog::Never;
use slog::Record;
use slog::SendSyncRefUnwindSafeDrain;
use slog_ext::make_tag_filter_drain;
use slog_glog_fmt::kv_categorizer::FacebookCategorizer;
use slog_glog_fmt::kv_defaults::FacebookKV;
use slog_glog_fmt::GlogFormat;
use slog_term::TermDecorator;
use sql_ext::facebook::MysqlOptions;
use sql_ext::facebook::PoolConfig;
use sql_ext::facebook::ReadConnectionType;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;
use tunables::init_tunables_worker;
use tunables::tunables;

pub type Normal = rand_distr::Normal<f64>;
use super::app::ArgType;
use super::app::MononokeAppData;
use super::app::ACL_FILE;
use super::app::BLOBSTORE_BYTES_MIN_THROTTLE_ARG;
use super::app::BLOBSTORE_PUT_BEHAVIOUR_ARG;
use super::app::BLOBSTORE_SCRUB_ACTION_ARG;
use super::app::BLOBSTORE_SCRUB_GRACE_ARG;
use super::app::BLOBSTORE_SCRUB_QUEUE_PEEK_BOUND_ARG;
use super::app::BLOBSTORE_SCRUB_WRITE_ONLY_MISSING_ARG;
use super::app::CACHELIB_ATTEMPT_ZSTD_ARG;
use super::app::CRYPTO_PATH_REGEX_ARG;
use super::app::DERIVE_REMOTELY;
use super::app::DERIVE_REMOTELY_TIER;
use super::app::DISABLE_TUNABLES;
use super::app::ENABLE_MCROUTER;
use super::app::GET_MEAN_DELAY_SECS_ARG;
use super::app::GET_STDDEV_DELAY_SECS_ARG;
use super::app::LOCAL_CONFIGERATOR_PATH_ARG;
use super::app::LOGVIEW_ADDITIONAL_LEVEL_FILTER;
use super::app::LOGVIEW_CATEGORY;
use super::app::LOG_EXCLUDE_TAG;
use super::app::LOG_INCLUDE_TAG;
use super::app::MYSQL_CONN_OPEN_TIMEOUT;
use super::app::MYSQL_MASTER_ONLY;
use super::app::MYSQL_MAX_QUERY_TIME;
use super::app::MYSQL_POOL_AGE_TIMEOUT;
use super::app::MYSQL_POOL_IDLE_TIMEOUT;
use super::app::MYSQL_POOL_LIMIT;
use super::app::MYSQL_POOL_PER_KEY_LIMIT;
use super::app::MYSQL_POOL_THREADS_NUM;
use super::app::MYSQL_SQLBLOB_POOL_AGE_TIMEOUT;
use super::app::MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT;
use super::app::MYSQL_SQLBLOB_POOL_LIMIT;
use super::app::MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT;
use super::app::MYSQL_SQLBLOB_POOL_THREADS_NUM;
use super::app::NO_DEFAULT_SCUBA_DATASET_ARG;
use super::app::PUT_MEAN_DELAY_SECS_ARG;
use super::app::PUT_STDDEV_DELAY_SECS_ARG;
use super::app::READ_BURST_BYTES_ARG;
use super::app::READ_BYTES_ARG;
use super::app::READ_CHAOS_ARG;
use super::app::READ_QPS_ARG;
use super::app::RENDEZVOUS_FREE_CONNECTIONS;
use super::app::RUNTIME_THREADS;
use super::app::SCUBA_DATASET_ARG;
use super::app::SCUBA_LOG_FILE_ARG;
use super::app::TUNABLES_CONFIG;
use super::app::TUNABLES_LOCAL_PATH;
use super::app::WARM_BOOKMARK_CACHE_SCUBA_DATASET_ARG;
use super::app::WITH_DYNAMIC_OBSERVABILITY;
use super::app::WITH_READONLY_STORAGE_ARG;
use super::app::WITH_TEST_MEGAREPO_CONFIGS_CLIENT;
use super::app::WRITE_BURST_BYTES_ARG;
use super::app::WRITE_BYTES_ARG;
use super::app::WRITE_CHAOS_ARG;
use super::app::WRITE_QPS_ARG;
use super::app::WRITE_ZSTD_ARG;
use super::app::WRITE_ZSTD_LEVEL_ARG;
use super::cache::parse_and_init_cachelib;
use crate::helpers::create_runtime;

trait Drain =
    slog::Drain<Ok = (), Err = Never> + Send + Sync + UnwindSafe + RefUnwindSafe + 'static;

const DEFAULT_TUNABLES_PATH: &str = "scm/mononoke/tunables/default";
const CRYPTO_PROJECT: &str = "SCM";

const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

pub struct MononokeMatches<'a> {
    matches: MaybeOwned<'a, ArgMatches<'a>>,
    app_data: MononokeAppData,
    environment: Arc<MononokeEnvironment>,
}

impl<'a> MononokeMatches<'a> {
    /// Due to global log init this can be called only once per process and will error otherwise
    pub fn new(
        fb: FacebookInit,
        matches: ArgMatches<'a>,
        app_data: MononokeAppData,
        arg_types: HashSet<ArgType>,
    ) -> Result<Self, Error> {
        let log_level = get_log_level(&matches);

        let log_filter_fn: Option<fn(&Record) -> bool> = app_data.slog_filter_fn;
        let root_log_drain = create_root_log_drain(fb, &matches, log_level, log_filter_fn)
            .context("Failed to create root log drain")?;
        #[cfg(fbcode_build)]
        cmdlib_logging::glog::set_glog_log_level(fb, log_level)?;

        // TODO: FacebookKV for this one?
        let config_store =
            create_config_store(fb, Logger::root(root_log_drain.clone(), o![]), &matches)
                .context("Failed to create config store")?;

        let observability_context =
            create_observability_context(&matches, &config_store, log_level)
                .context("Faled to initialize observability context")?;

        let logger = create_logger(&matches, root_log_drain, observability_context.clone())
            .context("Failed to create logger")?;
        let scuba_sample_builder =
            create_scuba_sample_builder(fb, &matches, &app_data, &observability_context)
                .context("Failed to create scuba sample builder")?;

        let warm_bookmarks_cache_scuba_sample_builder =
            create_warm_bookmark_cache_scuba_sample_builder(fb, &matches)
                .context("Failed to create warm bookmark cache scuba sample builder")?;

        let caching = parse_and_init_cachelib(fb, &matches, app_data.cachelib_settings.clone());

        let runtime = init_runtime(&matches).context("Failed to create Tokio runtime")?;

        init_tunables(
            &matches,
            &config_store,
            logger.clone(),
            runtime.handle().clone(),
        )
        .context("Failed to initialize tunables")?;

        let mysql_options =
            parse_mysql_options(&matches, &app_data).context("Failed to parse MySQL options")?;
        let blobstore_options = parse_blobstore_options(&matches, &app_data, &arg_types)
            .context("Failed to parse blobstore options")?;
        let readonly_storage =
            parse_readonly_storage(&matches).context("Failed to parse readonly storage options")?;
        let rendezvous_options =
            parse_rendezvous_options(&matches).context("Failed to parse rendezvous options")?;
        let megarepo_configs_options = parse_mononoke_megarepo_configs_options(&matches)?;
        let remote_derivation_options = parse_remote_derivation_options(&matches)?;
        let acl_provider = create_acl_provider(fb, &matches)?;

        maybe_enable_mcrouter(fb, &matches, &arg_types);

        Ok(MononokeMatches {
            matches: MaybeOwned::from(matches),
            environment: Arc::new(MononokeEnvironment {
                fb,
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
                acl_provider,
                rendezvous_options,
                megarepo_configs_options,
                remote_derivation_options,
                disabled_hooks: HashMap::new(),
                skiplist_enabled: true,
                warm_bookmarks_cache_derived_data: None,
                filter_repos: None,
            }),
            app_data,
        })
    }

    pub fn app_data(&self) -> &MononokeAppData {
        &self.app_data
    }

    pub fn environment(&self) -> &Arc<MononokeEnvironment> {
        &self.environment
    }

    pub fn caching(&self) -> Caching {
        self.environment.caching
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.environment.config_store
    }

    pub fn runtime(&self) -> &Handle {
        self.environment.runtime.handle()
    }

    pub fn logger(&self) -> &Logger {
        &self.environment.logger
    }

    pub fn mysql_options(&self) -> &MysqlOptions {
        &self.environment.mysql_options
    }

    pub fn blobstore_options(&self) -> &BlobstoreOptions {
        &self.environment.blobstore_options
    }

    pub fn readonly_storage(&self) -> &ReadOnlyStorage {
        &self.environment.readonly_storage
    }

    pub fn acl_provider(&self) -> &Arc<dyn AclProvider> {
        &self.environment.acl_provider
    }

    pub fn scuba_sample_builder(&self) -> MononokeScubaSampleBuilder {
        self.environment.scuba_sample_builder.clone()
    }

    pub fn warm_bookmarks_cache_scuba_sample_builder(&self) -> MononokeScubaSampleBuilder {
        self.environment
            .warm_bookmarks_cache_scuba_sample_builder
            .clone()
    }

    // Delegate some common methods to save on .as_ref() calls
    pub fn is_present<S: AsRef<str>>(&self, name: S) -> bool {
        self.matches.is_present(name)
    }

    pub fn subcommand(&'a self) -> (&str, Option<&'a ArgMatches<'a>>) {
        self.matches.subcommand()
    }

    pub fn usage(&self) -> &str {
        self.matches.usage()
    }

    pub fn value_of<S: AsRef<str>>(&self, name: S) -> Option<&str> {
        self.matches.value_of(name)
    }

    pub fn value_of_os<S: AsRef<str>>(&self, name: S) -> Option<&OsStr> {
        self.matches.value_of_os(name)
    }

    pub fn values_of<S: AsRef<str>>(&'a self, name: S) -> Option<Values<'a>> {
        self.matches.values_of(name)
    }

    pub fn values_of_os<S: AsRef<str>>(&'a self, name: S) -> Option<OsValues<'a>> {
        self.matches.values_of_os(name)
    }
}

impl<'a> AsRef<ArgMatches<'a>> for MononokeMatches<'a> {
    fn as_ref(&self) -> &ArgMatches<'a> {
        &self.matches
    }
}

impl<'a> Borrow<ArgMatches<'a>> for MononokeMatches<'a> {
    fn borrow(&self) -> &ArgMatches<'a> {
        &self.matches
    }
}

/// Create a default root logger for Facebook services
fn glog_drain() -> impl Drain {
    let decorator = TermDecorator::new().build();
    // FacebookCategorizer is used for slog KV arguments.
    // At the time of writing this code FacebookCategorizer and FacebookKV
    // that was added below was mainly useful for logview logging and had no effect on GlogFormat
    let drain = GlogFormat::new(decorator, FacebookCategorizer).ignore_res();
    ::std::sync::Mutex::new(drain).ignore_res()
}

fn get_log_level(matches: &ArgMatches<'_>) -> Level {
    if matches.is_present("debug") {
        Level::Debug
    } else {
        match matches.value_of("log-level") {
            Some(log_level_str) => Level::from_str(log_level_str)
                .unwrap_or_else(|_| panic!("Unknown log level: {}", log_level_str)),
            None => Level::Info,
        }
    }
}

fn create_root_log_drain(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
    log_level: Level,
    log_filter_fn: Option<fn(&Record) -> bool>,
) -> Result<impl Drain + Clone> {
    // Set the panic handler up here. Not really relevent to logger other than it emits output
    // when things go wrong. This writes directly to stderr as coredumper expects.
    let fate = match matches
        .value_of("panic-fate")
        .expect("no default on panic-fate")
    {
        "none" => None,
        "continue" => Some(Fate::Continue),
        "exit" => Some(Fate::Exit(101)),
        "abort" => Some(Fate::Abort),
        bad => bail!("bad panic-fate {}", bad),
    };
    if let Some(fate) = fate {
        panichandler::set_panichandler(fate);
    }

    let stdlog_env = "RUST_LOG";

    let glog_drain = make_tag_filter_drain(
        glog_drain(),
        matches
            .values_of(LOG_INCLUDE_TAG)
            .map(|v| v.map(|v| v.to_string()).collect())
            .unwrap_or_default(),
        matches
            .values_of(LOG_EXCLUDE_TAG)
            .map(|v| v.map(|v| v.to_string()).collect())
            .unwrap_or_default(),
        true, // Log messages which have no tags
    )?;

    let root_log_drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>> = match matches
        .value_of(LOGVIEW_CATEGORY)
    {
        Some(category) => {
            #[cfg(fbcode_build)]
            {
                // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
                // since logview is sampled anyway.
                let logview_drain = ::slog_logview::LogViewDrain::new(fb, category).ignore_res();
                match matches.value_of(LOGVIEW_ADDITIONAL_LEVEL_FILTER) {
                    Some(log_level_str) => {
                        let logview_level = Level::from_str(log_level_str)
                            .map_err(|_| format_err!("Unknown log level: {}", log_level_str))?;

                        let drain = slog::Duplicate::new(
                            glog_drain,
                            logview_drain.filter_level(logview_level).ignore_res(),
                        );
                        Arc::new(drain.ignore_res())
                    }
                    None => {
                        let drain = slog::Duplicate::new(glog_drain, logview_drain);
                        Arc::new(drain.ignore_res())
                    }
                }
            }
            #[cfg(not(fbcode_build))]
            {
                let _unused = LOGVIEW_ADDITIONAL_LEVEL_FILTER;
                let _ = (fb, category);
                unimplemented!(
                    "Passed --{}, but it is supported only for fbcode builds",
                    LOGVIEW_CATEGORY
                )
            }
        }
        None => Arc::new(glog_drain),
    };

    let root_log_drain = if let Some(filter_fn) = log_filter_fn {
        Arc::new(slog::IgnoreResult::new(slog::Filter::new(
            root_log_drain,
            filter_fn,
        )))
    } else {
        root_log_drain
    };

    // NOTE: We pass an unfiltered Logger to init_stdlog_once. That's because we do the filtering
    // at the stdlog level there.
    let stdlog_logger = Logger::root(root_log_drain.clone(), o![]);
    let stdlog_level = cmdlib_logging::log::init_stdlog_once(stdlog_logger, stdlog_env)?;

    // Note what level we enabled stdlog at, so that if someone is trying to debug they get
    // informed of potentially needing to set RUST_LOG.
    debug!(
        Logger::root(
            root_log_drain.clone().filter_level(log_level).ignore_res(),
            o![]
        ),
        "enabled stdlog with level: {:?} (set {} to configure)", stdlog_level, stdlog_env
    );

    Ok(root_log_drain)
}

fn create_logger(
    matches: &ArgMatches<'_>,
    root_log_drain: impl Drain + Clone,
    observability_context: ObservabilityContext,
) -> Result<Logger> {
    let root_log_drain = DynamicLevelDrain::new(root_log_drain, observability_context);

    let kv = FacebookKV::new().context("Failed to initialize FacebookKV")?;

    let logger = if matches.is_present("fb303-thrift-port") {
        Logger::root(slog_stats::StatsDrain::new(root_log_drain), o![kv])
    } else {
        Logger::root(root_log_drain, o![kv])
    };

    Ok(logger)
}

fn create_scuba_sample_builder(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
    app_data: &MononokeAppData,
    observability_context: &ObservabilityContext,
) -> Result<MononokeScubaSampleBuilder> {
    let mut scuba_logger = if let Some(scuba_dataset) = matches.value_of(SCUBA_DATASET_ARG) {
        MononokeScubaSampleBuilder::new(fb, scuba_dataset)?
    } else if let Some(default_scuba_dataset) = app_data.default_scuba_dataset.as_ref() {
        if matches.is_present(NO_DEFAULT_SCUBA_DATASET_ARG) {
            MononokeScubaSampleBuilder::with_discard()
        } else {
            MononokeScubaSampleBuilder::new(fb, default_scuba_dataset)?
        }
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };
    if let Some(scuba_log_file) = matches.value_of(SCUBA_LOG_FILE_ARG) {
        scuba_logger = scuba_logger.with_log_file(scuba_log_file)?;
    }
    let mut scuba_logger = scuba_logger
        .with_observability_context(observability_context.clone())
        .with_seq("seq");

    scuba_logger.add_common_server_data();

    Ok(scuba_logger)
}

fn create_warm_bookmark_cache_scuba_sample_builder(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
) -> Result<MononokeScubaSampleBuilder, Error> {
    let maybe_scuba = match matches
        .value_of(WARM_BOOKMARK_CACHE_SCUBA_DATASET_ARG)
        .map(|s| s.to_string())
    {
        Some(scuba) => {
            let hostname = hostname::get_hostname()?;
            let sampling_pct = tunables().get_warm_bookmark_cache_logging_sampling_pct() as u64;

            let mut hasher = DefaultHasher::new();
            hostname.hash(&mut hasher);

            if hasher.finish() % 100 < sampling_pct {
                Some(scuba)
            } else {
                None
            }
        }
        None => None,
    };

    MononokeScubaSampleBuilder::with_opt_table(fb, maybe_scuba)
}

fn parse_readonly_storage(matches: &ArgMatches<'_>) -> Result<ReadOnlyStorage, Error> {
    let ro = matches
        .value_of(WITH_READONLY_STORAGE_ARG)
        .map(|v| v.parse())
        .transpose()
        .with_context(|| format!("Provided {} is not bool", WITH_READONLY_STORAGE_ARG))?
        .unwrap_or(false);
    Ok(ReadOnlyStorage(ro))
}

fn parse_mysql_pool_options(matches: &ArgMatches<'_>) -> Result<PoolConfig, Error> {
    let size: usize = matches
        .value_of(MYSQL_POOL_LIMIT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-limit is not usize")?;
    let threads_num: i32 = matches
        .value_of(MYSQL_POOL_THREADS_NUM)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-threads-num is not i32")?;
    let per_key_limit: u64 = matches
        .value_of(MYSQL_POOL_PER_KEY_LIMIT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-per-key-limit is not u64")?;
    let conn_age_timeout: u64 = matches
        .value_of(MYSQL_POOL_AGE_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-age-timeout is not u64")?;
    let conn_idle_timeout: u64 = matches
        .value_of(MYSQL_POOL_IDLE_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-limit is not usize")?;
    let conn_open_timeout: u64 = matches
        .value_of(MYSQL_CONN_OPEN_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-conn-open-timeout is not u64")?;
    let max_query_time: Duration = Duration::from_millis(
        matches
            .value_of(MYSQL_MAX_QUERY_TIME)
            .expect("A default is set, should never be None")
            .parse()
            .context("Provided mysql-query-time-limit is not u64")?,
    );

    Ok(PoolConfig::new(
        size,
        threads_num,
        per_key_limit,
        conn_age_timeout,
        conn_idle_timeout,
        conn_open_timeout,
        max_query_time,
    ))
}

fn parse_mysql_options(
    matches: &ArgMatches<'_>,
    app_data: &MononokeAppData,
) -> Result<MysqlOptions, Error> {
    let pool = app_data.global_mysql_connection_pool.clone();
    let pool_config =
        parse_mysql_pool_options(matches).context("Failed to parse MySQL pool options")?;
    let read_connection_type = if matches.is_present(MYSQL_MASTER_ONLY) {
        ReadConnectionType::Master
    } else {
        ReadConnectionType::ReplicaOnly
    };

    Ok(MysqlOptions {
        pool,
        pool_config,
        read_connection_type,
    })
}

fn parse_mysql_sqlblob_pool_options(matches: &ArgMatches<'_>) -> Result<PoolConfig, Error> {
    let size: usize = matches
        .value_of(MYSQL_SQLBLOB_POOL_LIMIT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-limit is not usize")?;
    let threads_num: i32 = matches
        .value_of(MYSQL_SQLBLOB_POOL_THREADS_NUM)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-threads-num is not i32")?;
    let per_key_limit: u64 = matches
        .value_of(MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-per-key-limit is not u64")?;
    let conn_age_timeout: u64 = matches
        .value_of(MYSQL_SQLBLOB_POOL_AGE_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-age-timeout is not u64")?;
    let conn_idle_timeout: u64 = matches
        .value_of(MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-pool-limit is not usize")?;
    let conn_open_timeout: u64 = matches
        .value_of(MYSQL_CONN_OPEN_TIMEOUT)
        .expect("A default is set, should never be None")
        .parse()
        .context("Provided mysql-conn-open-timeout is not u64")?;
    let max_query_time: Duration = Duration::from_millis(
        matches
            .value_of(MYSQL_MAX_QUERY_TIME)
            .expect("A default is set, should never be None")
            .parse()
            .context("Provided mysql-query-time-limit is not u64")?,
    );

    Ok(PoolConfig::new(
        size,
        threads_num,
        per_key_limit,
        conn_age_timeout,
        conn_idle_timeout,
        conn_open_timeout,
        max_query_time,
    ))
}

fn parse_sqlblob_mysql_options(
    matches: &ArgMatches<'_>,
    app_data: &MononokeAppData,
) -> Result<MysqlOptions, Error> {
    let pool = app_data.sqlblob_mysql_connection_pool.clone();
    let pool_config = parse_mysql_sqlblob_pool_options(matches)?;
    let read_connection_type = if matches.is_present(MYSQL_MASTER_ONLY) {
        ReadConnectionType::Master
    } else {
        ReadConnectionType::ReplicaOnly
    };

    Ok(MysqlOptions {
        pool,
        pool_config,
        read_connection_type,
    })
}

fn parse_blobstore_options(
    matches: &ArgMatches<'_>,
    app_data: &MononokeAppData,
    arg_types: &HashSet<ArgType>,
) -> Result<BlobstoreOptions, Error> {
    let read_qps: Option<NonZeroU32> = matches
        .value_of(READ_QPS_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided qps is not u32")?;

    let write_qps: Option<NonZeroU32> = matches
        .value_of(WRITE_QPS_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided qps is not u32")?;

    let read_bytes: Option<NonZeroUsize> = matches
        .value_of(READ_BYTES_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Bytes/s is not usize")?;

    let write_bytes: Option<NonZeroUsize> = matches
        .value_of(WRITE_BYTES_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Bytes/s is not usize")?;

    let read_burst_bytes: Option<NonZeroUsize> = matches
        .value_of(READ_BURST_BYTES_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Bytes/s is not usize")?;

    let write_burst_bytes: Option<NonZeroUsize> = matches
        .value_of(WRITE_BURST_BYTES_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Bytes/s is not usize")?;

    let bytes_min_count: Option<NonZeroUsize> = matches
        .value_of(BLOBSTORE_BYTES_MIN_THROTTLE_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Bytes/s is not usize")?;

    let read_chaos: Option<NonZeroU32> = matches
        .value_of(READ_CHAOS_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided chaos is not u32")?;

    let write_chaos: Option<NonZeroU32> = matches
        .value_of(WRITE_CHAOS_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided chaos is not u32")?;

    #[cfg(fbcode_build)]
    let manifold_options = blobstore_factory::ManifoldOptions::parse_args(matches)?;

    let write_zstd: Option<bool> = matches
        .value_of(WRITE_ZSTD_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided value is not bool")?;

    let write_zstd_level: Option<i32> = matches
        .value_of(WRITE_ZSTD_LEVEL_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided Zstd compression level is not i32")?;

    let put_format_override = match (write_zstd, write_zstd_level) {
        (Some(false), Some(level)) => bail!(
            "Doesn't make sense to pass --{}=false with --{}={}",
            WRITE_ZSTD_ARG,
            WRITE_ZSTD_LEVEL_ARG,
            level
        ),
        (Some(false), None) => Some(PackFormat::Raw),
        (Some(true), None) => bail!(
            "When enabling --{} must also pass --{}",
            WRITE_ZSTD_ARG,
            WRITE_ZSTD_LEVEL_ARG
        ),
        (Some(true), Some(v)) => Some(PackFormat::ZstdIndividual(v)),
        (None, Some(level)) => bail!(
            "--{}={} requires --{}",
            WRITE_ZSTD_LEVEL_ARG,
            level,
            WRITE_ZSTD_ARG,
        ),
        (None, None) => None,
    };

    let attempt_zstd: bool = matches
        .value_of(CACHELIB_ATTEMPT_ZSTD_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided blobstore-cachelib-attempt-zstd is not bool")?
        .ok_or_else(|| format_err!("A default is set, should never be None"))?;

    let blobstore_put_behaviour: Option<PutBehaviour> = matches
        .value_of(BLOBSTORE_PUT_BEHAVIOUR_ARG)
        .map(|v| v.parse())
        .transpose()
        .context("Provided blobstore-put-behaviour is not PutBehaviour")?;

    let get_delay =
        parse_norm_distribution(matches, GET_MEAN_DELAY_SECS_ARG, GET_STDDEV_DELAY_SECS_ARG)?;
    let put_delay =
        parse_norm_distribution(matches, PUT_MEAN_DELAY_SECS_ARG, PUT_STDDEV_DELAY_SECS_ARG)?;

    let blobstore_options = BlobstoreOptions::new(
        ChaosOptions::new(read_chaos, write_chaos),
        DelayOptions {
            get_dist: get_delay,
            put_dist: put_delay,
        },
        ThrottleOptions {
            read_qps,
            write_qps,
            read_bytes,
            write_bytes,
            read_burst_bytes,
            write_burst_bytes,
            bytes_min_count,
        },
        #[cfg(fbcode_build)]
        manifold_options,
        PackOptions::new(put_format_override),
        CachelibBlobstoreOptions::new_lazy(Some(attempt_zstd)),
        blobstore_put_behaviour,
        parse_sqlblob_mysql_options(matches, app_data)
            .context("Failed to parse sqlblob MySQL options")?,
    );

    let blobstore_options = if arg_types.contains(&ArgType::Scrub) {
        let scrub_action = matches
            .value_of(BLOBSTORE_SCRUB_ACTION_ARG)
            .map(ScrubAction::from_str)
            .transpose()?;
        let scrub_grace = matches
            .value_of(BLOBSTORE_SCRUB_GRACE_ARG)
            .map(u64::from_str)
            .transpose()?;

        let scrub_action_on_missing_write_only = matches
            .value_of(BLOBSTORE_SCRUB_WRITE_ONLY_MISSING_ARG)
            .map(SrubWriteOnly::from_str)
            .transpose()?;
        let mut blobstore_options = blobstore_options
            .with_scrub_action(scrub_action)
            .with_scrub_grace(scrub_grace);
        if let Some(v) = scrub_action_on_missing_write_only {
            blobstore_options = blobstore_options.with_scrub_action_on_missing_write_only(v)
        }
        let scrub_queue_peek_bound = matches
            .value_of(BLOBSTORE_SCRUB_QUEUE_PEEK_BOUND_ARG)
            .map(u64::from_str)
            .transpose()?;
        if let Some(v) = scrub_queue_peek_bound {
            blobstore_options = blobstore_options.with_scrub_queue_peek_bound(v)
        }
        blobstore_options
    } else {
        blobstore_options
    };

    Ok(blobstore_options)
}

fn parse_norm_distribution(
    matches: &ArgMatches,
    mean_key: &str,
    stddev_key: &str,
) -> Result<Option<Normal>, Error> {
    let put_mean = crate::args::get_and_parse_opt(matches, mean_key);
    let put_stddev = crate::args::get_and_parse_opt(matches, stddev_key);
    match (put_mean, put_stddev) {
        (Some(put_mean), Some(put_stddev)) => {
            let dist = Normal::new(put_mean, put_stddev)
                .map_err(|err| format_err!("can't create normal distribution {:?}", err))?;
            Ok(Some(dist))
        }
        _ => Ok(None),
    }
}

fn parse_rendezvous_options(matches: &ArgMatches<'_>) -> Result<RendezVousOptions, Error> {
    let free_connections = matches
        .value_of(RENDEZVOUS_FREE_CONNECTIONS)
        .expect("A default is set, should never be None")
        .parse()
        .with_context(|| format!("Provided {} is not an integer", RENDEZVOUS_FREE_CONNECTIONS))?;
    Ok(RendezVousOptions { free_connections })
}

fn parse_mononoke_megarepo_configs_options(
    matches: &ArgMatches<'_>,
) -> Result<MononokeMegarepoConfigsOptions, Error> {
    let use_test: bool = matches
        .value_of(WITH_TEST_MEGAREPO_CONFIGS_CLIENT)
        .expect("A default is set, should never be None")
        .parse()
        .with_context(|| {
            format!(
                "Provided {} is not a bool",
                WITH_TEST_MEGAREPO_CONFIGS_CLIENT
            )
        })?;

    if use_test {
        if let Some(path) = matches.value_of(LOCAL_CONFIGERATOR_PATH_ARG) {
            Ok(MononokeMegarepoConfigsOptions::IntegrationTest(path.into()))
        } else {
            Ok(MononokeMegarepoConfigsOptions::UnitTest)
        }
    } else {
        Ok(MononokeMegarepoConfigsOptions::Prod)
    }
}

fn init_tunables<'a>(
    matches: &'a ArgMatches<'a>,
    config_store: &'a ConfigStore,
    logger: Logger,
    handle: Handle,
) -> Result<()> {
    if matches.is_present(DISABLE_TUNABLES) {
        debug!(logger, "Tunables are disabled");
        return Ok(());
    }

    if let Some(tunables_local_path) = matches.value_of(TUNABLES_LOCAL_PATH) {
        let value = std::fs::read_to_string(tunables_local_path)
            .with_context(|| format!("failed to open tunables path {}", tunables_local_path))?;
        let config_handle = ConfigHandle::from_json(&value)
            .with_context(|| format!("failed to parse tunables at path {}", tunables_local_path))?;
        return tunables::init_tunables(&logger, &config_handle);
    }

    let tunables_spec = matches
        .value_of(TUNABLES_CONFIG)
        .unwrap_or(DEFAULT_TUNABLES_PATH);

    let config_handle =
        config_store.get_config_handle(parse_config_spec_to_path(tunables_spec)?)?;

    init_tunables_worker(logger, config_handle, handle)
}

/// Initialize a new `Runtime` with thread number parsed from the CLI
fn init_runtime(matches: &ArgMatches<'_>) -> Result<Runtime> {
    let core_threads = matches
        .value_of(RUNTIME_THREADS)
        .map(|v| v.parse())
        .transpose()
        .with_context(|| format!("Failed to parse {}", RUNTIME_THREADS))?;
    let rt = create_runtime(None, core_threads).context("Failed to create runtime")?;
    Ok(rt)
}

fn create_observability_context<'a>(
    matches: &'a ArgMatches<'a>,
    config_store: &'a ConfigStore,
    log_level: Level,
) -> Result<ObservabilityContext, Error> {
    match matches.value_of(WITH_DYNAMIC_OBSERVABILITY) {
        Some("true") => Ok(ObservabilityContext::new(config_store)?),
        Some("false") | None => Ok(ObservabilityContext::new_static(log_level)),
        Some(other) => panic!(
            "Unexpected --{} value: {}",
            WITH_DYNAMIC_OBSERVABILITY, other
        ),
    }
}

fn create_config_store<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'a>,
) -> Result<ConfigStore, Error> {
    let local_configerator_path = matches.value_of(LOCAL_CONFIGERATOR_PATH_ARG);
    let crypto_regex = matches.values_of(CRYPTO_PATH_REGEX_ARG).map_or(
        vec![
            (
                "scm/mononoke/tunables/.*".to_string(),
                CRYPTO_PROJECT.to_string(),
            ),
            (
                "scm/mononoke/repos/.*".to_string(),
                CRYPTO_PROJECT.to_string(),
            ),
            (
                "scm/mononoke/redaction/.*".to_string(),
                CRYPTO_PROJECT.to_string(),
            ),
            (
                "scm/mononoke/lfs_server/.*".to_string(),
                CRYPTO_PROJECT.to_string(),
            ),
        ],
        |it| {
            it.map(|regex| (regex.to_string(), CRYPTO_PROJECT.to_string()))
                .collect()
        },
    );
    match local_configerator_path {
        // A local configerator path wins
        Some(path) => Ok(ConfigStore::file(
            logger,
            PathBuf::from(path),
            String::new(),
            CONFIGERATOR_POLL_INTERVAL,
        )),
        // Prod instances do have network configerator, with signature checks
        None => ConfigStore::regex_signed_configerator(
            fb,
            logger,
            crypto_regex,
            CONFIGERATOR_POLL_INTERVAL,
            CONFIGERATOR_REFRESH_TIMEOUT,
        ),
    }
}

fn maybe_enable_mcrouter(fb: FacebookInit, matches: &ArgMatches<'_>, arg_types: &HashSet<ArgType>) {
    if !arg_types.contains(&ArgType::McRouter) {
        return;
    }

    if !matches.is_present(ENABLE_MCROUTER) {
        return;
    }

    #[cfg(fbcode_build)]
    {
        ::ratelim::use_proxy_if_available(fb);
    }

    #[cfg(not(fbcode_build))]
    {
        let _ = fb;
        unimplemented!(
            "Passed --{}, but it is supported only for fbcode builds",
            ENABLE_MCROUTER
        );
    }
}

fn parse_remote_derivation_options(
    matches: &ArgMatches<'_>,
) -> Result<RemoteDerivationOptions, Error> {
    let derive_remotely = matches.is_present(DERIVE_REMOTELY);
    let smc_tier = matches
        .value_of(DERIVE_REMOTELY_TIER)
        .map(|s| s.to_string());
    Ok(RemoteDerivationOptions {
        derive_remotely,
        smc_tier,
    })
}

fn create_acl_provider(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
) -> Result<Arc<dyn AclProvider>, Error> {
    match matches.value_of(ACL_FILE) {
        Some(file) => InternalAclProvider::from_file(file),
        None => Ok(DefaultAclProvider::new(fb)),
    }
}
