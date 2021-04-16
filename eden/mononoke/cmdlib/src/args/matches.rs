/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, format_err, Context, Error, Result};
use cached_config::ConfigStore;
use clap::{ArgMatches, Values};
use fbinit::FacebookInit;
use maybe_owned::MaybeOwned;
use panichandler::{self, Fate};
use slog::{debug, o, trace, Level, Logger, Never, SendSyncRefUnwindSafeDrain};
use slog_glog_fmt::{kv_categorizer::FacebookCategorizer, kv_defaults::FacebookKV, GlogFormat};
use slog_term::TermDecorator;
use std::panic::{RefUnwindSafe, UnwindSafe};
use tokio::runtime::{Handle, Runtime};

use blobstore_factory::{
    BlobstoreOptions, CachelibBlobstoreOptions, ChaosOptions, PackOptions, PutBehaviour,
    ScrubAction, ThrottleOptions,
};
use environment::{Caching, MononokeEnvironment};
use metaconfig_types::PackFormat;
use observability::{DynamicLevelDrain, ObservabilityContext};
use repo_factory::ReadOnlyStorage;
use scuba_ext::MononokeScubaSampleBuilder;
use slog_ext::make_tag_filter_drain;
use sql_ext::facebook::{MysqlConnectionType, MysqlOptions, PoolConfig};
use tunables::init_tunables_worker;

use crate::helpers::create_runtime;
use crate::log;

use super::app::{
    ArgType, MononokeAppData, BLOBSTORE_BYTES_MIN_THROTTLE_ARG, BLOBSTORE_PUT_BEHAVIOUR_ARG,
    BLOBSTORE_SCRUB_ACTION_ARG, BLOBSTORE_SCRUB_GRACE_ARG, CACHELIB_ATTEMPT_ZSTD_ARG,
    CRYPTO_PATH_REGEX_ARG, DISABLE_TUNABLES, LOCAL_CONFIGERATOR_PATH_ARG, LOG_EXCLUDE_TAG,
    LOG_INCLUDE_TAG, MANIFOLD_API_KEY_ARG, MYSQL_CONN_OPEN_TIMEOUT, MYSQL_MASTER_ONLY,
    MYSQL_MAX_QUERY_TIME, MYSQL_MYROUTER_PORT, MYSQL_POOL_AGE_TIMEOUT, MYSQL_POOL_IDLE_TIMEOUT,
    MYSQL_POOL_LIMIT, MYSQL_POOL_PER_KEY_LIMIT, MYSQL_POOL_THREADS_NUM,
    MYSQL_SQLBLOB_POOL_AGE_TIMEOUT, MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT, MYSQL_SQLBLOB_POOL_LIMIT,
    MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT, MYSQL_SQLBLOB_POOL_THREADS_NUM, MYSQL_USE_CLIENT,
    READ_BURST_BYTES_ARG, READ_BYTES_ARG, READ_CHAOS_ARG, READ_QPS_ARG, RUNTIME_THREADS,
    TUNABLES_CONFIG, WITH_DYNAMIC_OBSERVABILITY, WITH_READONLY_STORAGE_ARG, WRITE_BURST_BYTES_ARG,
    WRITE_BYTES_ARG, WRITE_CHAOS_ARG, WRITE_QPS_ARG, WRITE_ZSTD_ARG, WRITE_ZSTD_LEVEL_ARG,
};
use super::cache::parse_and_init_cachelib;
use super::parse_config_spec_to_path;

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
    pub fn new(
        fb: FacebookInit,
        matches: ArgMatches<'a>,
        app_data: MononokeAppData,
        arg_types: HashSet<ArgType>,
    ) -> Result<Self, Error> {
        let root_log_drain =
            create_root_log_drain(fb, &matches).context("Failed to create root log drain")?;

        // TODO: FacebookKV for this one?
        let config_store =
            create_config_store(fb, Logger::root(root_log_drain.clone(), o![]), &matches)
                .context("Failed to create config store")?;

        let observability_context = create_observability_context(&matches, &config_store)
            .context("Faled to initialize observability context")?;

        let logger = create_logger(&matches, root_log_drain, observability_context.clone())
            .context("Failed to create logger")?;

        let caching = parse_and_init_cachelib(fb, &matches, app_data.cachelib_settings.clone());

        let runtime = init_runtime(&matches).context("Failed to create Tokio runtime")?;

        init_tunables(&matches, &config_store, logger.clone())
            .context("Failed to initialize tunables")?;

        let mysql_options =
            parse_mysql_options(&matches, &app_data).context("Failed to parse MySQL options")?;
        let blobstore_options = parse_blobstore_options(&matches, &app_data, &arg_types)
            .context("Failed to parse blobstore options")?;
        let readonly_storage =
            parse_readonly_storage(&matches).context("Failed to parse readonly storage options")?;

        Ok(MononokeMatches {
            matches: MaybeOwned::from(matches),
            environment: Arc::new(MononokeEnvironment {
                fb,
                logger,
                config_store,
                caching,
                observability_context,
                runtime,
                mysql_options,
                blobstore_options,
                readonly_storage,
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
        &self.environment.runtime.handle()
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

    pub fn scuba_sample_builder(&self) -> Result<MononokeScubaSampleBuilder> {
        let mut scuba_logger = if let Some(scuba_dataset) = self.value_of("scuba-dataset") {
            MononokeScubaSampleBuilder::new(self.environment.fb, scuba_dataset)
        } else if let Some(default_scuba_dataset) = self.app_data.default_scuba_dataset.as_ref() {
            if self.is_present("no-default-scuba-dataset") {
                MononokeScubaSampleBuilder::with_discard()
            } else {
                MononokeScubaSampleBuilder::new(self.environment.fb, default_scuba_dataset)
            }
        } else {
            MononokeScubaSampleBuilder::with_discard()
        };
        if let Some(scuba_log_file) = self.value_of("scuba-log-file") {
            scuba_logger = scuba_logger.with_log_file(scuba_log_file)?;
        }
        let scuba_logger = scuba_logger
            .with_observability_context(self.environment.observability_context.clone())
            .with_seq("seq");


        // TODO: add_common_server_data?

        Ok(scuba_logger)
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

fn create_root_log_drain(fb: FacebookInit, matches: &ArgMatches<'_>) -> Result<impl Drain + Clone> {
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
        .value_of("logview-category")
    {
        Some(category) => {
            #[cfg(fbcode_build)]
            {
                // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
                // since logview is sampled anyway.
                let logview_drain = ::slog_logview::LogViewDrain::new(fb, category).ignore_res();
                let drain = slog::Duplicate::new(glog_drain, logview_drain);
                Arc::new(drain.ignore_res())
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = (fb, category);
                unimplemented!(
                    "Passed --logview-category, but it is supported only for fbcode builds"
                )
            }
        }
        None => Arc::new(glog_drain),
    };

    // NOTE: We pass an unfiltered Logger to init_stdlog_once. That's because we do the filtering
    // at the stdlog level there.
    let stdlog_logger = Logger::root(root_log_drain.clone(), o![]);
    let stdlog_level = log::init_stdlog_once(stdlog_logger.clone(), stdlog_env);
    trace!(
        stdlog_logger,
        "enabled stdlog with level: {:?} (set {} to configure)",
        stdlog_level,
        stdlog_env
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
    let connection_type = if let Some(port) = matches.value_of(MYSQL_MYROUTER_PORT) {
        let port = port
            .parse::<u16>()
            .context("Provided --myrouter-port is not u16")?;
        MysqlConnectionType::Myrouter(port)
    } else if matches.is_present(MYSQL_USE_CLIENT) {
        let pool = app_data.global_mysql_connection_pool.clone();
        let pool_config =
            parse_mysql_pool_options(matches).context("Failed to parse MySQL pool options")?;

        MysqlConnectionType::Mysql(pool, pool_config)
    } else {
        MysqlConnectionType::RawXDB
    };

    let master_only = matches.is_present(MYSQL_MASTER_ONLY);

    Ok(MysqlOptions {
        connection_type,
        master_only,
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
    let connection_type = if let Some(port) = matches.value_of(MYSQL_MYROUTER_PORT) {
        let port = port
            .parse::<u16>()
            .context("Provided --myrouter-port is not u16")?;
        MysqlConnectionType::Myrouter(port)
    } else if matches.is_present(MYSQL_USE_CLIENT) {
        let pool = app_data.sqlblob_mysql_connection_pool.clone();
        let pool_config = parse_mysql_sqlblob_pool_options(matches)?;

        MysqlConnectionType::Mysql(pool, pool_config)
    } else {
        MysqlConnectionType::RawXDB
    };

    let master_only = matches.is_present(MYSQL_MASTER_ONLY);

    Ok(MysqlOptions {
        connection_type,
        master_only,
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

    let manifold_api_key: Option<String> = matches
        .value_of(MANIFOLD_API_KEY_ARG)
        .map(|api_key| api_key.to_string());

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

    let blobstore_options = BlobstoreOptions::new(
        ChaosOptions::new(read_chaos, write_chaos),
        ThrottleOptions {
            read_qps,
            write_qps,
            read_bytes,
            write_bytes,
            read_burst_bytes,
            write_burst_bytes,
            bytes_min_count,
        },
        manifold_api_key,
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
        blobstore_options
            .with_scrub_action(scrub_action)
            .with_scrub_grace(scrub_grace)
    } else {
        blobstore_options
    };

    Ok(blobstore_options)
}

fn init_tunables<'a>(
    matches: &'a ArgMatches<'a>,
    config_store: &'a ConfigStore,
    logger: Logger,
) -> Result<()> {
    if matches.is_present(DISABLE_TUNABLES) {
        debug!(logger, "Tunables are disabled");
        return Ok(());
    }

    let tunables_spec = matches
        .value_of(TUNABLES_CONFIG)
        .unwrap_or(DEFAULT_TUNABLES_PATH);

    let config_handle =
        config_store.get_config_handle(parse_config_spec_to_path(tunables_spec)?)?;

    init_tunables_worker(logger, config_handle)
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
) -> Result<ObservabilityContext, Error> {
    match matches.value_of(WITH_DYNAMIC_OBSERVABILITY) {
        Some("true") => Ok(ObservabilityContext::new(config_store)?),
        Some("false") | None => Ok(ObservabilityContext::new_static(get_log_level(matches))),
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
