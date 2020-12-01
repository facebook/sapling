/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cache;
#[cfg(fbcode_build)]
mod facebook;

pub use self::cache::init_cachelib;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io;
use std::iter::FromIterator;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, format_err, Error, Result};
use cached_config::{ConfigHandle, ConfigStore, TestSource};
use clap::{App, Arg, ArgGroup, ArgMatches};
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use once_cell::sync::OnceCell;
use panichandler::{self, Fate};
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{debug, info, o, warn, Drain, Level, Logger, Never, SendSyncRefUnwindSafeDrain};
use slog_term::TermDecorator;

use slog_glog_fmt::{kv_categorizer::FacebookCategorizer, kv_defaults::FacebookKV, GlogFormat};

use blobrepo::BlobRepo;
use blobrepo_factory::{BlobrepoBuilder, Caching, ReadOnlyStorage};
use blobstore_factory::{
    BlobstoreOptions, CachelibBlobstoreOptions, ChaosOptions, PackOptions, PutBehaviour, Scrubbing,
    ThrottleOptions, DEFAULT_PUT_BEHAVIOUR,
};
use metaconfig_parser::{RepoConfigs, StorageConfigs};
use metaconfig_types::{BlobConfig, CommonConfig, Redaction, RepoConfig, ScrubAction};
use mononoke_types::RepositoryId;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::{MysqlConnectionType, MysqlOptions};
use tunables::init_tunables_worker;

use crate::helpers::{
    create_runtime, open_sql_with_config_and_mysql_options, setup_repo_dir, CreateStorage,
};
use crate::log;

use self::cache::{add_cachelib_args, parse_caching};

const CONFIG_PATH: &str = "mononoke-config-path";
const REPO_ID: &str = "repo-id";
const REPO_NAME: &str = "repo-name";
const SOURCE_REPO_GROUP: &str = "source-repo";
const SOURCE_REPO_ID: &str = "source-repo-id";
const SOURCE_REPO_NAME: &str = "source-repo-name";
const TARGET_REPO_GROUP: &str = "target-repo";
const TARGET_REPO_ID: &str = "target-repo-id";
const TARGET_REPO_NAME: &str = "target-repo-name";
const ENABLE_MCROUTER: &str = "enable-mcrouter";
const MYSQL_MYROUTER_PORT: &str = "myrouter-port";
const MYSQL_MASTER_ONLY: &str = "mysql-master-only";
const MYSQL_USE_CLIENT: &str = "use-mysql-client";
const RUNTIME_THREADS: &str = "runtime-threads";
const TUNABLES_CONFIG: &str = "tunables-config";
const DISABLE_TUNABLES: &str = "disable-tunables";

const DEFAULT_TUNABLES_PATH: &str = "configerator:scm/mononoke/tunables/default";

const READ_QPS_ARG: &str = "blobstore-read-qps";
const WRITE_QPS_ARG: &str = "blobstore-write-qps";
const READ_CHAOS_ARG: &str = "blobstore-read-chaos-rate";
const WRITE_CHAOS_ARG: &str = "blobstore-write-chaos-rate";
const WRITE_ZSTD_ARG: &str = "blobstore-write-zstd-level";
const MANIFOLD_API_KEY_ARG: &str = "manifold-api-key";
const CACHELIB_ATTEMPT_ZSTD_ARG: &str = "blobstore-cachelib-attempt-zstd";
const BLOBSTORE_PUT_BEHAVIOUR_ARG: &str = "blobstore-put-behaviour";
const TEST_INSTANCE_ARG: &str = "test-instance";
const LOCAL_CONFIGERATOR_PATH_ARG: &str = "local-configerator-path";
const CRYPTO_PATH_REGEX_ARG: &str = "crypto-path-regex";

const CRYPTO_PROJECT: &str = "SCM";

const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgType {
    /// Options related to mononoke config
    Config,
    /// Options related to mononoke tests
    Test,
    /// Options related to stderr logging,
    Logging,
    /// Adds options related to mysql database connections
    Mysql,
    /// Adds options related to blobstore access
    Blobstore,
    /// Adds options related to cachelib and its use from blobstore
    Cachelib,
    /// Adds options related to tokio runtime
    Runtime,
    /// Adds options related to mononoke tunables
    Tunables,
    /// Adds options to select a repo. If not present then all repos.
    Repo,
    /// Adds --source-repo-id/repo-name and --target-repo-id/repo-name options.
    /// Necessary for crossrepo operations
    /// Only visible if Repo group is visible.
    SourceAndTargetRepos,
    /// Adds just --source-repo-id/repo-name, for blobimport into a megarepo
    /// Only visible if Repo group is visible.
    SourceRepo,
    /// Adds --shutdown-grace-period and --shutdown-timeout for graceful shutdown.
    ShutdownTimeouts,
    /// Adds --scuba-dataset and --scuba-log-file for scuba logging.
    ScubaLogging,
    /// Adds --disabled-hooks for disabling hooks.
    DisableHooks,
    /// Adds --fb303-thrift-port for stats and profiling
    Fb303,
}

// Arguments that are enabled by default for MononokeApp
const DEFAULT_ARG_TYPES: &[ArgType] = &[
    ArgType::Blobstore,
    ArgType::Cachelib,
    ArgType::Config,
    ArgType::Logging,
    ArgType::Mysql,
    ArgType::Repo,
    ArgType::Runtime,
    ArgType::Test,
    ArgType::Tunables,
];

pub struct MononokeApp {
    /// The app name.
    name: String,

    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    hide_advanced_args: bool,

    /// Whether to require the user select a repo if the option is present.
    repo_required: bool,

    /// Which groups of arguments are enabled for this app
    arg_types: HashSet<ArgType>,

    /// This app is special admin tool, needs to run with specific PutBehaviour
    special_put_behaviour: Option<PutBehaviour>,
}

/// Create a default root logger for Facebook services
pub fn glog_drain() -> impl Drain<Ok = (), Err = Never> {
    let decorator = TermDecorator::new().build();
    // FacebookCategorizer is used for slog KV arguments.
    // At the time of writing this code FacebookCategorizer and FacebookKV
    // that was added below was mainly useful for logview logging and had no effect on GlogFormat
    let drain = GlogFormat::new(decorator, FacebookCategorizer).ignore_res();
    ::std::sync::Mutex::new(drain).ignore_res()
}

impl MononokeApp {
    /// Start building a new Mononoke app.  This adds the standard Mononoke args.  Use the `build`
    /// method to get a `clap::App` that you can then customize further.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hide_advanced_args: false,
            repo_required: false,
            arg_types: HashSet::from_iter(DEFAULT_ARG_TYPES.iter().cloned()),
            special_put_behaviour: None,
        }
    }

    /// Hide advanced args.
    pub fn with_advanced_args_hidden(mut self) -> Self {
        self.hide_advanced_args = true;
        self
    }

    /// This command operates on all configured repos, and removes the options for selecting a
    /// repo.  The default behaviour is for the arguments to specify the repo to be optional, which is
    /// probably not what you want, so you should call either this method or `with_repo_required`.
    pub fn with_all_repos(mut self) -> Self {
        self.arg_types.remove(&ArgType::Repo);
        self
    }

    /// This command operates on a specific repos, so this makes the options for selecting a
    /// repo required.  The default behaviour is for the arguments to specify the repo to be
    /// optional, which is probably not what you want, so you should call either this method or
    /// `with_all_repos`.
    pub fn with_repo_required(mut self) -> Self {
        self.arg_types.insert(ArgType::Repo);
        self.repo_required = true;
        self
    }

    /// This command might operate on two repos in the same time. This is normally used
    /// for two repos where one repo is synced into another.
    pub fn with_source_and_target_repos(mut self) -> Self {
        self.arg_types.insert(ArgType::SourceAndTargetRepos);
        self
    }

    /// This command operates on one repo (--repo-id/name), but needs to be aware that commits
    /// are sourced from another repo.
    pub fn with_source_repos(mut self) -> Self {
        self.arg_types.insert(ArgType::SourceRepo);
        self
    }

    /// This command has arguments for graceful shutdown.
    pub fn with_shutdown_timeout_args(mut self) -> Self {
        self.arg_types.insert(ArgType::ShutdownTimeouts);
        self
    }

    /// This command has arguments for scuba logging.
    pub fn with_scuba_logging_args(mut self) -> Self {
        self.arg_types.insert(ArgType::ScubaLogging);
        self
    }

    /// This command has arguments for disabled hooks.
    pub fn with_disabled_hooks_args(mut self) -> Self {
        self.arg_types.insert(ArgType::DisableHooks);
        self
    }

    /// This command has arguments for fb303
    pub fn with_fb303_args(mut self) -> Self {
        self.arg_types.insert(ArgType::Fb303);
        self
    }

    /// This command does expose these types of arguments
    pub fn with_arg_types<I>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = ArgType>,
    {
        for t in types {
            self.arg_types.insert(t);
        }
        self
    }

    /// This command does not expose these types of arguments
    pub fn without_arg_types<I>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = ArgType>,
    {
        for t in types {
            self.arg_types.remove(&t);
        }
        self
    }

    /// This command needs a special default put behaviour (e.g. its an admin tool)
    pub fn with_special_put_behaviour(mut self, put_behaviour: PutBehaviour) -> Self {
        self.special_put_behaviour = Some(put_behaviour);
        self
    }

    /// Build a `clap::App` for this Mononoke app, which can then be customized further.
    pub fn build<'a, 'b>(self) -> App<'a, 'b> {
        let mut app = App::new(self.name);

        if self.arg_types.contains(&ArgType::Config) {
            app = app.arg(
                Arg::with_name(CONFIG_PATH)
                    .long(CONFIG_PATH)
                    .value_name("MONONOKE_CONFIG_PATH")
                    .help("Path to the Mononoke configs"),
            )
            .arg(
                Arg::with_name(CRYPTO_PATH_REGEX_ARG)
                    .multiple(true)
                    .long(CRYPTO_PATH_REGEX_ARG)
                    .takes_value(true)
                    .help("Regex for a Configerator path that must be covered by Mononoke's crypto project")
            )
            .arg(
                Arg::with_name(LOCAL_CONFIGERATOR_PATH_ARG)
                    .long(LOCAL_CONFIGERATOR_PATH_ARG)
                    .takes_value(true)
                    .help("local path to fetch configerator configs from, instead of normal configerator"),
            );
        }

        if self.arg_types.contains(&ArgType::Test) {
            app = app.arg(
                Arg::with_name(TEST_INSTANCE_ARG)
                    .long(TEST_INSTANCE_ARG)
                    .takes_value(false)
                    .help("disables some functionality for tests"),
            );
        }

        if self.arg_types.contains(&ArgType::Repo) {
            let repo_conflicts: &[&str] = if self.arg_types.contains(&ArgType::SourceRepo) {
                &[TARGET_REPO_ID, TARGET_REPO_NAME]
            } else {
                &[
                    SOURCE_REPO_ID,
                    SOURCE_REPO_NAME,
                    TARGET_REPO_ID,
                    TARGET_REPO_NAME,
                ]
            };

            app = app
                .arg(
                    Arg::with_name(REPO_ID)
                        .long(REPO_ID)
                        // This is an old form that some consumers use
                        .alias("repo_id")
                        .value_name("ID")
                        .help("numeric ID of repository")
                        .conflicts_with_all(repo_conflicts),
                )
                .arg(
                    Arg::with_name(REPO_NAME)
                        .long(REPO_NAME)
                        .value_name("NAME")
                        .help("Name of repository")
                        .conflicts_with_all(repo_conflicts),
                )
                .group(
                    ArgGroup::with_name("repo")
                        .args(&[REPO_ID, REPO_NAME])
                        .required(self.repo_required),
                );

            if self.arg_types.contains(&ArgType::SourceRepo)
                || self.arg_types.contains(&ArgType::SourceAndTargetRepos)
            {
                app = app
                    .arg(
                        Arg::with_name(SOURCE_REPO_ID)
                        .long(SOURCE_REPO_ID)
                        .value_name("ID")
                        .help("numeric ID of source repository (used only for commands that operate on more than one repo)"),
                    )
                    .arg(
                        Arg::with_name(SOURCE_REPO_NAME)
                        .long(SOURCE_REPO_NAME)
                        .value_name("NAME")
                        .help("Name of source repository (used only for commands that operate on more than one repo)"),
                    )
                    .group(
                        ArgGroup::with_name(SOURCE_REPO_GROUP)
                            .args(&[SOURCE_REPO_ID, SOURCE_REPO_NAME])
                    )
            }

            if self.arg_types.contains(&ArgType::SourceAndTargetRepos) {
                app = app
                    .arg(
                        Arg::with_name(TARGET_REPO_ID)
                        .long(TARGET_REPO_ID)
                        .value_name("ID")
                        .help("numeric ID of target repository (used only for commands that operate on more than one repo)"),
                    )
                    .arg(
                        Arg::with_name(TARGET_REPO_NAME)
                        .long(TARGET_REPO_NAME)
                        .value_name("NAME")
                        .help("Name of target repository (used only for commands that operate on more than one repo)"),
                    )
                    .group(
                        ArgGroup::with_name(TARGET_REPO_GROUP)
                            .args(&[TARGET_REPO_ID, TARGET_REPO_NAME])
                    );
            }
        }

        if self.arg_types.contains(&ArgType::Logging) {
            app = add_logger_args(app);
        }
        if self.arg_types.contains(&ArgType::Mysql) {
            app = add_mysql_options_args(app);
        }
        if self.arg_types.contains(&ArgType::Blobstore) {
            app = add_blobstore_args(app, self.special_put_behaviour);
        }
        if self.arg_types.contains(&ArgType::Cachelib) {
            app = add_cachelib_args(app, self.hide_advanced_args);
        }
        if self.arg_types.contains(&ArgType::Runtime) {
            app = add_runtime_args(app);
        }
        if self.arg_types.contains(&ArgType::Tunables) {
            app = add_tunables_args(app);
        }
        if self.arg_types.contains(&ArgType::ShutdownTimeouts) {
            app = add_shutdown_timeout_args(app);
        }
        if self.arg_types.contains(&ArgType::ScubaLogging) {
            app = add_scuba_logging_args(app);
        }
        if self.arg_types.contains(&ArgType::DisableHooks) {
            app = add_disabled_hooks_args(app);
        }
        if self.arg_types.contains(&ArgType::Fb303) {
            app = add_fb303_args(app);
        }

        app
    }
}

fn add_tunables_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(TUNABLES_CONFIG)
            .long(TUNABLES_CONFIG)
            .takes_value(true)
            .help("The location of a tunables config"),
    )
    .arg(
        Arg::with_name(DISABLE_TUNABLES)
            .long(DISABLE_TUNABLES)
            .help("Use the default values for all tunables (useful for tests)"),
    )
}
fn add_runtime_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(RUNTIME_THREADS)
            .long(RUNTIME_THREADS)
            .takes_value(true)
            .help("a number of threads to use in the tokio runtime"),
    )
}

fn add_logger_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("panic-fate")
            .long("panic-fate")
            .value_name("PANIC_FATE")
            .possible_values(&["continue", "exit", "abort"])
            .default_value("abort")
            .help("fate of the process when a panic happens"),
    )
    .arg(
        Arg::with_name("logview-category")
            .long("logview-category")
            .takes_value(true)
            .help("logview category to log to. Logview is not used if not set"),
    )
    .arg(
        Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("print debug output"),
    )
    .arg(
        Arg::with_name("log-level")
            .long("log-level")
            .help("log level to use (does not work with --debug)")
            .takes_value(true)
            .possible_values(&["CRITICAL", "ERROR", "WARN", "INFO", "DEBUG", "TRACE"])
            .conflicts_with("debug"),
    )
}

pub fn init_logging<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) -> Logger {
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
        bad => panic!("bad panic-fate {}", bad),
    };
    if let Some(fate) = fate {
        panichandler::set_panichandler(fate);
    }

    let stdlog_env = "RUST_LOG";

    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        match matches.value_of("log-level") {
            Some(log_level_str) => Level::from_str(log_level_str)
                .expect(&format!("Unknown log level: {}", log_level_str)),
            None => Level::Info,
        }
    };

    let glog_drain = Arc::new(glog_drain());
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
        None => Arc::new(glog_drain.ignore_res()),
    };

    // NOTE: We pass an unfitlered Logger to init_stdlog_once. That's because we do the filtering
    // at the stdlog level there.
    let stdlog_level =
        log::init_stdlog_once(Logger::root(root_log_drain.clone(), o![]), stdlog_env);

    let root_log_drain = root_log_drain.filter_level(level).ignore_res();

    let kv = FacebookKV::new().expect("cannot initialize FacebookKV");

    let logger = if matches.is_present("fb303-thrift-port") {
        Logger::root(slog_stats::StatsDrain::new(root_log_drain), o![kv])
    } else {
        Logger::root(root_log_drain, o![kv])
    };

    debug!(
        logger,
        "enabled stdlog with level: {:?} (set {} to configure)", stdlog_level, stdlog_env
    );

    logger
}

fn get_repo_id_and_name_from_values<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
    option_repo_name: &str,
    option_repo_id: &str,
) -> Result<(RepositoryId, String)> {
    let repo_name = matches.value_of(option_repo_name);
    let repo_id = matches.value_of(option_repo_id);
    let configs = load_repo_configs(config_store, matches)?;

    match (repo_name, repo_id) {
        (Some(_), Some(_)) => bail!("both repo-name and repo-id parameters set"),
        (None, None) => bail!("neither repo-name nor repo-id parameter set"),
        (None, Some(repo_id)) => {
            let repo_id = RepositoryId::from_str(repo_id)?;
            let mut repo_config: Vec<_> = configs
                .repos
                .into_iter()
                .filter(|(_, repo_config)| repo_config.repoid == repo_id)
                .collect();
            if repo_config.is_empty() {
                Err(format_err!("unknown config for repo-id {:?}", repo_id))
            } else if repo_config.len() > 1 {
                Err(format_err!(
                    "multiple configs defined for repo-id {:?}",
                    repo_id
                ))
            } else {
                let (repo_name, repo_config) = repo_config.pop().unwrap();
                Ok((repo_config.repoid, repo_name))
            }
        }
        (Some(repo_name), None) => {
            let mut repo_config: Vec<_> = configs
                .repos
                .into_iter()
                .filter(|(name, _)| name == repo_name)
                .collect();
            if repo_config.is_empty() {
                Err(format_err!("unknown repo-name {:?}", repo_name))
            } else if repo_config.len() > 1 {
                Err(format_err!(
                    "multiple configs defined for repo-name {:?}",
                    repo_name
                ))
            } else {
                let (repo_name, repo_config) = repo_config.pop().unwrap();
                Ok((repo_config.repoid, repo_name))
            }
        }
    }
}

pub fn get_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) = get_repo_id_and_name_from_values(config_store, matches, REPO_NAME, REPO_ID)?;
    Ok(repo_id)
}

pub fn get_repo_name<'a>(config_store: &ConfigStore, matches: &ArgMatches<'a>) -> Result<String> {
    let (_, repo_name) =
        get_repo_id_and_name_from_values(config_store, matches, REPO_NAME, REPO_ID)?;
    Ok(repo_name)
}

pub fn get_source_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) =
        get_repo_id_and_name_from_values(config_store, matches, SOURCE_REPO_NAME, SOURCE_REPO_ID)?;
    Ok(repo_id)
}

pub fn get_source_repo_id_opt<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<Option<RepositoryId>> {
    if matches.is_present(SOURCE_REPO_NAME) || matches.is_present(SOURCE_REPO_ID) {
        let (repo_id, _) = get_repo_id_and_name_from_values(
            config_store,
            matches,
            SOURCE_REPO_NAME,
            SOURCE_REPO_ID,
        )?;
        Ok(Some(repo_id))
    } else {
        Ok(None)
    }
}

pub fn get_target_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) =
        get_repo_id_and_name_from_values(config_store, matches, TARGET_REPO_NAME, TARGET_REPO_ID)?;
    Ok(repo_id)
}

pub fn get_repo_id_from_value<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
    repo_id_arg: &str,
) -> Result<RepositoryId> {
    let (repo_id, _) = get_repo_id_and_name_from_values(config_store, matches, "", repo_id_arg)?;
    Ok(repo_id)
}

pub async fn open_sql<T>(
    fb: FacebookInit,
    config_store: &ConfigStore,
    matches: &ArgMatches<'_>,
) -> Result<T, Error>
where
    T: SqlConstructFromMetadataDatabaseConfig,
{
    let (_, config) = get_config(config_store, matches)?;
    let mysql_options = parse_mysql_options(matches);
    let readonly_storage = parse_readonly_storage(matches);
    open_sql_with_config_and_mysql_options(
        fb,
        config.storage_config.metadata,
        mysql_options,
        readonly_storage,
    )
    .compat()
    .await
}

pub async fn open_source_sql<T>(
    fb: FacebookInit,
    config_store: &ConfigStore,
    matches: &ArgMatches<'_>,
) -> Result<T, Error>
where
    T: SqlConstructFromMetadataDatabaseConfig,
{
    let source_repo_id = get_source_repo_id(config_store, matches)?;
    let (_, config) = get_config_by_repoid(config_store, matches, source_repo_id)?;
    let mysql_options = parse_mysql_options(matches);
    let readonly_storage = parse_readonly_storage(matches);
    open_sql_with_config_and_mysql_options(
        fb,
        config.storage_config.metadata,
        mysql_options,
        readonly_storage,
    )
    .compat()
    .await
}

/// Create a new `BlobRepo` -- for local instances, expect its contents to be empty.
#[inline]
pub fn create_repo<'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a ArgMatches<'a>,
) -> impl Future<Output = Result<BlobRepo, Error>> + 'a {
    open_repo_internal(
        fb,
        logger,
        matches,
        true,
        parse_caching(matches),
        Scrubbing::Disabled,
        None,
    )
}

/// Create a new `BlobRepo` -- for local instances, expect its contents to be empty.
/// Make sure that the opened repo has redaction disabled
#[inline]
pub fn create_repo_unredacted<'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a ArgMatches<'a>,
) -> impl Future<Output = Result<BlobRepo, Error>> + 'a {
    open_repo_internal(
        fb,
        logger,
        matches,
        true,
        parse_caching(matches),
        Scrubbing::Disabled,
        Some(Redaction::Disabled),
    )
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
#[inline]
pub fn open_repo<'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a ArgMatches<'a>,
) -> impl Future<Output = Result<BlobRepo, Error>> + 'a {
    open_repo_internal(
        fb,
        logger,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Disabled,
        None,
    )
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
/// Make sure that the opened repo has redaction disabled
#[inline]
pub fn open_repo_unredacted<'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a ArgMatches<'a>,
) -> impl Future<Output = Result<BlobRepo, Error>> + 'a {
    open_repo_internal(
        fb,
        logger,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Disabled,
        Some(Redaction::Disabled),
    )
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
/// If there are multiple backing blobstores, open them in scrub mode, where we check that
/// the blobstore contents all match.
#[inline]
pub async fn open_scrub_repo<'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a ArgMatches<'a>,
) -> impl Future<Output = Result<BlobRepo, Error>> + 'a {
    open_repo_internal(
        fb,
        logger,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Enabled,
        None,
    )
}

fn add_mysql_options_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(MYSQL_MYROUTER_PORT)
            .long(MYSQL_MYROUTER_PORT)
            .help("Use MyRouter at this port")
            .takes_value(true),
    )
    .arg(
        Arg::with_name(MYSQL_MASTER_ONLY)
            .long(MYSQL_MASTER_ONLY)
            .help("Connect to MySQL master only")
            .takes_value(false),
    )
    .arg(
        Arg::with_name(MYSQL_USE_CLIENT)
            .long(MYSQL_USE_CLIENT)
            .help("Connect via Mysql client")
            .takes_value(false)
            .conflicts_with(MYSQL_MYROUTER_PORT),
    )
}

fn add_blobstore_args<'a, 'b>(
    app: App<'a, 'b>,
    special_put_behaviour: Option<PutBehaviour>,
) -> App<'a, 'b> {
    let mut put_arg = Arg::with_name(BLOBSTORE_PUT_BEHAVIOUR_ARG)
        .long(BLOBSTORE_PUT_BEHAVIOUR_ARG)
        .takes_value(true)
        .required(false)
        .help("Desired blobstore behaviour when a put is made to an existing key.");

    if let Some(special_put_behaviour) = special_put_behaviour {
        put_arg = put_arg.default_value(special_put_behaviour.into());
    } else {
        // Add the default here so that it shows in --help
        put_arg = put_arg.default_value(DEFAULT_PUT_BEHAVIOUR.into());
    }

    app.arg(
        Arg::with_name(READ_QPS_ARG)
            .long(READ_QPS_ARG)
            .takes_value(true)
            .required(false)
            .help("Read QPS limit to ThrottledBlob"),
    )
    .arg(
        Arg::with_name(WRITE_QPS_ARG)
            .long(WRITE_QPS_ARG)
            .takes_value(true)
            .required(false)
            .help("Write QPS limit to ThrottledBlob"),
    )
    .arg(
        Arg::with_name(READ_CHAOS_ARG)
            .long(READ_CHAOS_ARG)
            .takes_value(true)
            .required(false)
            .help("Rate of errors on reads. Pass N,  it will error randomly 1/N times. For multiplexed stores will only apply to the first store in the multiplex."),
    )
    .arg(
        Arg::with_name(WRITE_CHAOS_ARG)
            .long(WRITE_CHAOS_ARG)
            .takes_value(true)
            .required(false)
            .help("Rate of errors on writes. Pass N,  it will error randomly 1/N times. For multiplexed stores will only apply to the first store in the multiplex."),
    )
    .arg(
        Arg::with_name(WRITE_ZSTD_ARG)
            .long(WRITE_ZSTD_ARG)
            .takes_value(true)
            .required(false)
            .help("Set the zstd compression level to be used on writes via the packed blobstore (if configured).  Default is None."),
    )
    .arg(
        Arg::with_name(MANIFOLD_API_KEY_ARG)
            .long(MANIFOLD_API_KEY_ARG)
            .takes_value(true)
            .required(false)
            .help("Manifold API key"),
    )
    .arg(
        Arg::with_name(CACHELIB_ATTEMPT_ZSTD_ARG)
            .long(CACHELIB_ATTEMPT_ZSTD_ARG)
            .takes_value(true)
            .required(false)
            .help("Whether to attempt zstd compression when blobstore is putting things into cachelib over threshold size.  Default is true."),
    )
    .arg(
      put_arg
    )
}

pub fn add_mcrouter_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ENABLE_MCROUTER)
            .long(ENABLE_MCROUTER)
            .help("Use local McRouter for rate limits")
            .takes_value(false),
    )
}

pub(crate) fn add_fb303_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.args_from_usage(r"--fb303-thrift-port=[PORT]    'port for fb303 service'")
}

fn add_disabled_hooks_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("disabled-hooks")
            .long("disable-hook")
            .help("Disable a hook. Pass this argument multiple times to disable multiple hooks.")
            .multiple(true)
            .number_of_values(1)
            .takes_value(true),
    )
}

fn add_shutdown_timeout_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("shutdown-grace-period")
            .long("shutdown-grace-period")
            .help(
                "Number of seconds to wait after receiving a shutdown signal before shutting down.",
            )
            .takes_value(true)
            .required(false)
            .default_value("0"),
    )
    .arg(
        Arg::with_name("shutdown-timeout")
            .long("shutdown-timeout")
            .help("Number of seconds to wait for requests to complete during shutdown.")
            .takes_value(true)
            .required(false)
            .default_value("10"),
    )
}

pub fn get_shutdown_grace_period<'a>(matches: &ArgMatches<'a>) -> Result<Duration> {
    let seconds = matches
        .value_of("shutdown-grace-period")
        .ok_or(Error::msg("shutdown-grace-period must be specified"))?
        .parse()
        .map_err(Error::from)?;
    Ok(Duration::from_secs(seconds))
}

pub fn get_shutdown_timeout<'a>(matches: &ArgMatches<'a>) -> Result<Duration> {
    let seconds = matches
        .value_of("shutdown-timeout")
        .ok_or(Error::msg("shutdown-timeout must be specified"))?
        .parse()
        .map_err(Error::from)?;
    Ok(Duration::from_secs(seconds))
}

fn add_scuba_logging_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("scuba-dataset")
            .long("scuba-dataset")
            .takes_value(true)
            .help("The name of the scuba dataset to log to"),
    )
    .arg(
        Arg::with_name("scuba-log-file")
            .long("scuba-log-file")
            .takes_value(true)
            .help("A log file to write Scuba logs to (primarily useful in testing)"),
    )
}

pub fn get_scuba_sample_builder<'a>(
    fb: FacebookInit,
    matches: &ArgMatches<'a>,
) -> Result<MononokeScubaSampleBuilder> {
    let mut scuba_logger = if let Some(scuba_dataset) = matches.value_of("scuba-dataset") {
        MononokeScubaSampleBuilder::new(fb, scuba_dataset)
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };
    if let Some(scuba_log_file) = matches.value_of("scuba-log-file") {
        scuba_logger = scuba_logger.with_log_file(scuba_log_file)?;
    }
    let scuba_logger = scuba_logger.with_seq("seq");
    Ok(scuba_logger)
}

pub fn add_scribe_logging_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("scribe-logging-directory")
            .long("scribe-logging-directory")
            .takes_value(true)
            .help("Filesystem directory where to log all scribe writes"),
    )
}

pub fn get_scribe<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) -> Result<Scribe> {
    match matches.value_of("scribe-logging-directory") {
        Some(dir) => Ok(Scribe::new_to_file(PathBuf::from(dir))),
        None => Ok(Scribe::new(fb)),
    }
}

pub fn get_config_path<'a>(matches: &'a ArgMatches<'a>) -> Result<&'a str> {
    matches
        .value_of(CONFIG_PATH)
        .ok_or(Error::msg(format!("{} must be specified", CONFIG_PATH)))
}

pub fn load_repo_configs<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<RepoConfigs> {
    metaconfig_parser::load_repo_configs(get_config_path(matches)?, config_store)
}

pub fn load_common_config<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<CommonConfig> {
    metaconfig_parser::load_common_config(get_config_path(matches)?, config_store)
}

pub fn load_storage_configs<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<StorageConfigs> {
    metaconfig_parser::load_storage_configs(get_config_path(matches)?, config_store)
}

pub fn get_config<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
) -> Result<(String, RepoConfig)> {
    let repo_id = get_repo_id(config_store, matches)?;
    get_config_by_repoid(config_store, matches, repo_id)
}

pub fn get_config_by_repoid<'a>(
    config_store: &ConfigStore,
    matches: &ArgMatches<'a>,
    repo_id: RepositoryId,
) -> Result<(String, RepoConfig)> {
    let configs = load_repo_configs(config_store, matches)?;
    configs
        .get_repo_config(repo_id)
        .ok_or_else(|| format_err!("unknown repoid {:?}", repo_id))
        .map(|(name, config)| (name.clone(), config.clone()))
}

async fn open_repo_internal(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'_>,
    create: bool,
    caching: Caching,
    scrub: Scrubbing,
    redaction_override: Option<Redaction>,
) -> Result<BlobRepo, Error> {
    let config_store = init_config_store(fb, logger, matches)?;
    let repo_id = get_repo_id(config_store, matches)?;
    open_repo_internal_with_repo_id(
        fb,
        logger,
        repo_id,
        matches,
        create,
        caching,
        scrub,
        redaction_override,
    )
    .await
}

async fn open_repo_internal_with_repo_id(
    fb: FacebookInit,
    logger: &Logger,
    repo_id: RepositoryId,
    matches: &ArgMatches<'_>,
    create: bool,
    caching: Caching,
    scrub: Scrubbing,
    redaction_override: Option<Redaction>,
) -> Result<BlobRepo, Error> {
    let config_store = init_config_store(fb, logger, matches)?;
    let common_config = load_common_config(config_store, &matches)?;

    let (reponame, config) = {
        let (reponame, mut config) = get_config_by_repoid(config_store, matches, repo_id)?;
        if let Scrubbing::Enabled = scrub {
            config
                .storage_config
                .blobstore
                .set_scrubbed(ScrubAction::ReportOnly);
        }
        (reponame, config)
    };
    info!(logger, "using repo \"{}\" repoid {:?}", reponame, repo_id);
    match &config.storage_config.blobstore {
        BlobConfig::Files { path } | BlobConfig::Sqlite { path } => {
            let create = if create {
                // Many path repos can share one blobstore, so allow store to exist or create it.
                CreateStorage::ExistingOrCreate
            } else {
                CreateStorage::ExistingOnly
            };
            setup_repo_dir(path, create)?;
        }
        _ => {}
    };

    let mysql_options = parse_mysql_options(matches);
    let blobstore_options = parse_blobstore_options(matches);
    let readonly_storage = parse_readonly_storage(matches);

    let mut builder = BlobrepoBuilder::new(
        fb,
        reponame,
        &config,
        mysql_options,
        caching,
        common_config.censored_scuba_params,
        readonly_storage,
        blobstore_options,
        &logger,
        config_store,
    );
    if let Some(redaction_override) = redaction_override {
        builder.set_redaction(redaction_override);
    }
    builder.build().await
}

pub async fn open_repo_with_repo_id(
    fb: FacebookInit,
    logger: &Logger,
    repo_id: RepositoryId,
    matches: &ArgMatches<'_>,
) -> Result<BlobRepo, Error> {
    open_repo_internal_with_repo_id(
        fb,
        logger,
        repo_id,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Disabled,
        None,
    )
    .await
}

pub fn parse_readonly_storage<'a>(matches: &ArgMatches<'a>) -> ReadOnlyStorage {
    ReadOnlyStorage(matches.is_present("readonly-storage"))
}

pub fn parse_mysql_options<'a>(matches: &ArgMatches<'a>) -> MysqlOptions {
    let connection_type = if let Some(port) = matches.value_of(MYSQL_MYROUTER_PORT) {
        let port = port
            .parse::<u16>()
            .expect("Provided --myrouter-port is not u16");
        MysqlConnectionType::Myrouter(port)
    } else if matches.is_present(MYSQL_USE_CLIENT) {
        MysqlConnectionType::Mysql
    } else {
        MysqlConnectionType::RawXDB
    };

    let master_only = matches.is_present(MYSQL_MASTER_ONLY);

    MysqlOptions {
        connection_type,
        master_only,
    }
}

pub fn parse_blobstore_options<'a>(matches: &ArgMatches<'a>) -> BlobstoreOptions {
    let read_qps: Option<NonZeroU32> = matches
        .value_of(READ_QPS_ARG)
        .map(|v| v.parse().expect("Provided qps is not u32"));

    let write_qps: Option<NonZeroU32> = matches
        .value_of(WRITE_QPS_ARG)
        .map(|v| v.parse().expect("Provided qps is not u32"));

    let read_chaos: Option<NonZeroU32> = matches
        .value_of(READ_CHAOS_ARG)
        .map(|v| v.parse().expect("Provided chaos is not u32"));

    let write_chaos: Option<NonZeroU32> = matches
        .value_of(WRITE_CHAOS_ARG)
        .map(|v| v.parse().expect("Provided chaos is not u32"));

    let manifold_api_key: Option<String> = matches
        .value_of(MANIFOLD_API_KEY_ARG)
        .map(|api_key| api_key.to_string());

    let write_zstd_level: Option<i32> = matches.value_of(WRITE_ZSTD_ARG).map(|v| {
        v.parse()
            .expect("Provided Zstd compression level is not i32")
    });

    let attempt_zstd: Option<bool> = matches.value_of(CACHELIB_ATTEMPT_ZSTD_ARG).map(|v| {
        v.parse()
            .expect("Provided blobstore-cachelib-attempt-zstd is not bool")
    });


    let blobstore_put_behaviour: Option<PutBehaviour> =
        matches.value_of(BLOBSTORE_PUT_BEHAVIOUR_ARG).map(|v| {
            v.parse()
                .expect("Provided blobstore-put-behaviour is not PutBehaviour")
        });

    BlobstoreOptions::new(
        ChaosOptions::new(read_chaos, write_chaos),
        ThrottleOptions::new(read_qps, write_qps),
        manifold_api_key,
        PackOptions::new(write_zstd_level),
        CachelibBlobstoreOptions::new_lazy(attempt_zstd),
        blobstore_put_behaviour,
    )
}

pub fn maybe_enable_mcrouter<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) {
    if matches.is_present(ENABLE_MCROUTER) {
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
}

pub fn get_usize_opt<'a>(matches: &ArgMatches<'a>, key: &str) -> Option<usize> {
    matches.value_of(key).map(|val| {
        val.parse::<usize>()
            .expect(&format!("{} must be integer", key))
    })
}

#[inline]
pub fn get_usize<'a>(matches: &ArgMatches<'a>, key: &str, default: usize) -> usize {
    get_usize_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_u64<'a>(matches: &ArgMatches<'a>, key: &str, default: u64) -> u64 {
    get_u64_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_and_parse_opt<'a, T: ::std::str::FromStr>(
    matches: &ArgMatches<'a>,
    key: &str,
) -> Option<T>
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .value_of(key)
        .map(|val| val.parse::<T>().expect(&format!("{} - invalid value", key)))
}

#[inline]
pub fn get_and_parse<'a, T: ::std::str::FromStr>(
    matches: &ArgMatches<'a>,
    key: &str,
    default: T,
) -> T
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    get_and_parse_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_u64_opt<'a>(matches: &ArgMatches<'a>, key: &str) -> Option<u64> {
    matches.value_of(key).map(|val| {
        val.parse::<u64>()
            .expect(&format!("{} must be integer", key))
    })
}

#[inline]
pub fn get_i32_opt<'a>(matches: &ArgMatches<'a>, key: &str) -> Option<i32> {
    matches.value_of(key).map(|val| {
        val.parse::<i32>()
            .expect(&format!("{} must be integer", key))
    })
}

#[inline]
pub fn get_i32<'a>(matches: &ArgMatches<'a>, key: &str, default: i32) -> i32 {
    get_i32_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_i64_opt<'a>(matches: &ArgMatches<'a>, key: &str) -> Option<i64> {
    matches.value_of(key).map(|val| {
        val.parse::<i64>()
            .expect(&format!("{} must be integer", key))
    })
}

pub fn parse_disabled_hooks_with_repo_prefix(
    matches: &ArgMatches,
    logger: &Logger,
) -> Result<HashMap<String, HashSet<String>>, Error> {
    let disabled_hooks = matches
        .values_of("disabled-hooks")
        .map(|m| m.collect())
        .unwrap_or(vec![]);

    let mut res = HashMap::new();
    for repohook in disabled_hooks {
        let repohook: Vec<_> = repohook.splitn(2, ":").collect();
        let repo = repohook.get(0);
        let hook = repohook.get(1);

        let (repo, hook) =
            repo.and_then(|repo| hook.map(|hook| (repo, hook)))
                .ok_or(format_err!(
                    "invalid format of disabled hook, should be 'REPONAME:HOOKNAME'"
                ))?;
        res.entry(repo.to_string())
            .or_insert(HashSet::new())
            .insert(hook.to_string());
    }
    if !res.is_empty() {
        warn!(logger, "The following Hooks were disabled: {:?}", res);
    }
    Ok(res)
}

pub fn parse_disabled_hooks_no_repo_prefix(
    matches: &ArgMatches,
    logger: &Logger,
) -> HashSet<String> {
    let disabled_hooks: HashSet<String> = matches
        .values_of("disabled-hooks")
        .map(|m| m.collect())
        .unwrap_or(vec![])
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    if !disabled_hooks.is_empty() {
        warn!(
            logger,
            "The following Hooks were disabled: {:?}", disabled_hooks
        );
    }

    disabled_hooks
}

pub fn init_mononoke<'a>(
    fb: FacebookInit,
    matches: &ArgMatches<'a>,
    expected_item_size_bytes: Option<usize>,
) -> Result<(Caching, Logger, tokio_compat::runtime::Runtime)> {
    let logger = init_logging(fb, matches);

    debug!(logger, "Initialising cachelib...");
    let caching = init_cachelib(fb, matches, expected_item_size_bytes);
    debug!(logger, "Initialising runtime...");
    let runtime = init_runtime(matches)?;
    init_tunables(fb, matches, logger.clone())?;

    Ok((caching, logger, runtime))
}

pub fn init_tunables<'a>(fb: FacebookInit, matches: &ArgMatches<'a>, logger: Logger) -> Result<()> {
    if matches.is_present(DISABLE_TUNABLES) {
        debug!(logger, "Tunables are disabled");
        return Ok(());
    }

    let config_store = init_config_store(fb, &logger, matches)?;

    let tunables_spec = matches
        .value_of(TUNABLES_CONFIG)
        .unwrap_or(DEFAULT_TUNABLES_PATH);

    let config_handle = get_config_handle(config_store, &logger, Some(tunables_spec))?;

    init_tunables_worker(logger, config_handle)
}
/// Initialize a new `tokio_compat::runtime::Runtime` with thread number parsed from the CLI
pub fn init_runtime(matches: &ArgMatches) -> io::Result<tokio_compat::runtime::Runtime> {
    let core_threads = get_usize_opt(matches, RUNTIME_THREADS);
    create_runtime(None, core_threads)
}

/// Extract a ConfigHandle<T> from a source_spec str that has one ofthe folowing formats:
/// - configerator:PATH
/// - file:PATH
/// - default
/// NB: Outside tests, using file:PATH is not recommended because it is inefficient - instead
/// use a local configerator path and configerator:PATH
pub fn get_config_handle<T>(
    config_store: &ConfigStore,
    logger: &Logger,
    source_spec: Option<&str>,
) -> Result<ConfigHandle<T>, Error>
where
    T: Default + Send + Sync + 'static + serde::de::DeserializeOwned,
{
    match source_spec {
        Some(source_spec) => {
            // NOTE: This means we don't support file paths with ":" in them, but it also means we can
            // add other options after the first ":" later if we want.
            let mut iter = source_spec.split(":");

            // NOTE: We match None as the last element to make sure the input doesn't contain
            // disallowed trailing parts.
            match (iter.next(), iter.next(), iter.next()) {
                (Some("configerator"), Some(source), None) => {
                    config_store.get_config_handle(source.to_string())
                }
                (Some("file"), Some(file), None) => ConfigStore::file(
                    logger.clone(),
                    PathBuf::new(),
                    String::new(),
                    Duration::from_secs(1),
                )
                .get_config_handle(file.to_string()),
                (Some("default"), None, None) => Ok(ConfigHandle::default()),
                _ => Err(format_err!("Invalid configuration spec: {:?}", source_spec)),
            }
        }
        None => Ok(ConfigHandle::default()),
    }
}

static CONFIGERATOR: OnceCell<ConfigStore> = OnceCell::new();

pub fn is_test_instance<'a>(matches: &ArgMatches<'a>) -> bool {
    matches.is_present(TEST_INSTANCE_ARG)
}

pub fn init_config_store<'a>(
    fb: FacebookInit,
    root_log: impl Into<Option<&'a Logger>>,
    matches: &ArgMatches<'a>,
) -> Result<&'static ConfigStore, Error> {
    CONFIGERATOR.get_or_try_init(|| {
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
        match (is_test_instance(matches), local_configerator_path) {
            // A local configerator path wins
            (_, Some(path)) => Ok(ConfigStore::file(
                root_log.into().cloned(),
                PathBuf::from(path),
                String::new(),
                CONFIGERATOR_POLL_INTERVAL,
            )),
            // Test instances can't have network configerator
            (true, None) => Ok(ConfigStore::new(Arc::new(TestSource::new()), None, None)),
            // Prod instances do have network configerator, with signature checks
            (false, None) => ConfigStore::regex_signed_configerator(
                fb,
                root_log.into().cloned(),
                crypto_regex,
                CONFIGERATOR_POLL_INTERVAL,
                CONFIGERATOR_REFRESH_TIMEOUT,
            ),
        }
    })
}
