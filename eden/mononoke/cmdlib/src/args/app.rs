/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::ffi::OsString;
use std::num::NonZeroU32;

use anyhow::Error;
use anyhow::Result;
use blobstore_factory::PutBehaviour;
use blobstore_factory::ScrubAction;
use blobstore_factory::SrubWriteOnly;
use blobstore_factory::DEFAULT_PUT_BEHAVIOUR;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgGroup;
use fbinit::FacebookInit;
use once_cell::sync::OnceCell;
use repo_factory::ReadOnlyStorage;
use slog::Record;
use sql_ext::facebook::SharedConnectionPool;
use strum::VariantNames;

use super::cache::add_cachelib_args;
use super::cache::CachelibSettings;
use super::matches::MononokeMatches;

pub const CONFIG_PATH: &str = "mononoke-config-path";
pub const REPO_ID: &str = "repo-id";
pub const REPO_NAME: &str = "repo-name";
pub const SHARDED_SERVICE_NAME: &str = "sharded-service-name";
pub const SOURCE_REPO_GROUP: &str = "source-repo";
pub const SOURCE_REPO_ID: &str = "source-repo-id";
pub const SOURCE_REPO_NAME: &str = "source-repo-name";
pub const TARGET_REPO_GROUP: &str = "target-repo";
pub const TARGET_REPO_ID: &str = "target-repo-id";
pub const TARGET_REPO_NAME: &str = "target-repo-name";
pub const ENABLE_MCROUTER: &str = "enable-mcrouter";
pub const MYSQL_MASTER_ONLY: &str = "mysql-master-only";
pub const MYSQL_POOL_LIMIT: &str = "mysql-pool-limit";
pub const MYSQL_POOL_PER_KEY_LIMIT: &str = "mysql-pool-per-key-limit";
pub const MYSQL_POOL_THREADS_NUM: &str = "mysql-pool-threads-num";
pub const MYSQL_POOL_AGE_TIMEOUT: &str = "mysql-pool-age-timeout";
pub const MYSQL_POOL_IDLE_TIMEOUT: &str = "mysql-pool-idle-timeout";
pub const MYSQL_CONN_OPEN_TIMEOUT: &str = "mysql-conn-open-timeout";
pub const MYSQL_MAX_QUERY_TIME: &str = "mysql-query-time-limit";
pub const MYSQL_SQLBLOB_POOL_LIMIT: &str = "mysql-sqblob-pool-limit";
pub const MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT: &str = "mysql-sqblob-pool-per-key-limit";
pub const MYSQL_SQLBLOB_POOL_THREADS_NUM: &str = "mysql-sqblob-pool-threads-num";
pub const MYSQL_SQLBLOB_POOL_AGE_TIMEOUT: &str = "mysql-sqblob-pool-age-timeout";
pub const MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT: &str = "mysql-sqblob-pool-idle-timeout";
pub const RUNTIME_THREADS: &str = "runtime-threads";
pub const TUNABLES_CONFIG: &str = "tunables-config";
pub const TUNABLES_LOCAL_PATH: &str = "tunables-local-path";
pub const DISABLE_TUNABLES: &str = "disable-tunables";
pub const SCRIBE_LOGGING_DIRECTORY: &str = "scribe-logging-directory";
pub const RENDEZVOUS_FREE_CONNECTIONS: &str = "rendezvous-free-connections";

pub const READ_QPS_ARG: &str = "blobstore-read-qps";
pub const WRITE_QPS_ARG: &str = "blobstore-write-qps";
pub const READ_BYTES_ARG: &str = "blobstore-read-bytes-s";
pub const WRITE_BYTES_ARG: &str = "blobstore-write-bytes-s";
pub const READ_BURST_BYTES_ARG: &str = "blobstore-read-burst-bytes-s";
pub const WRITE_BURST_BYTES_ARG: &str = "blobstore-write-burst-bytes-s";
pub const BLOBSTORE_BYTES_MIN_THROTTLE_ARG: &str = "blobstore-bytes-min-throttle";
pub const READ_CHAOS_ARG: &str = "blobstore-read-chaos-rate";
pub const WRITE_CHAOS_ARG: &str = "blobstore-write-chaos-rate";
pub const WRITE_ZSTD_ARG: &str = "blobstore-write-zstd";
pub const WRITE_ZSTD_LEVEL_ARG: &str = "blobstore-write-zstd-level";
pub const CACHELIB_ATTEMPT_ZSTD_ARG: &str = "blobstore-cachelib-attempt-zstd";
pub const BLOBSTORE_PUT_BEHAVIOUR_ARG: &str = "blobstore-put-behaviour";
pub const BLOBSTORE_SCRUB_ACTION_ARG: &str = "blobstore-scrub-action";
pub const BLOBSTORE_SCRUB_GRACE_ARG: &str = "blobstore-scrub-grace";
pub const BLOBSTORE_SCRUB_WRITE_ONLY_MISSING_ARG: &str = "blobstore-scrub-write-only-missing";
pub const BLOBSTORE_SCRUB_QUEUE_PEEK_BOUND_ARG: &str = "blobstore-scrub-queue-peek";
pub const PUT_MEAN_DELAY_SECS_ARG: &str = "blobstore-put-mean-delay-secs";
pub const PUT_STDDEV_DELAY_SECS_ARG: &str = "blobstore-put-stddev-delay-secs";
pub const GET_MEAN_DELAY_SECS_ARG: &str = "blobstore-get-mean-delay-secs";
pub const GET_STDDEV_DELAY_SECS_ARG: &str = "blobstore-get-stddev-delay-secs";

pub const WITH_READONLY_STORAGE_ARG: &str = "with-readonly-storage";

pub const LOG_INCLUDE_TAG: &str = "log-include-tag";
pub const LOG_EXCLUDE_TAG: &str = "log-exclude-tag";
pub const LOGVIEW_CATEGORY: &str = "logview-category";
pub const LOGVIEW_ADDITIONAL_LEVEL_FILTER: &str = "logview-additional-level-filter";
pub const SCUBA_DATASET_ARG: &str = "scuba-dataset";
pub const SCUBA_LOG_FILE_ARG: &str = "scuba-log-file";
pub const NO_DEFAULT_SCUBA_DATASET_ARG: &str = "no-default-scuba-dataset";
pub const WARM_BOOKMARK_CACHE_SCUBA_DATASET_ARG: &str = "warm-bookmark-cache-scuba-dataset";

// Argument, responsible for instantiation of `ObservabilityContext::Dynamic`
pub const WITH_DYNAMIC_OBSERVABILITY: &str = "with-dynamic-observability";

pub const LOCAL_CONFIGERATOR_PATH_ARG: &str = "local-configerator-path";
pub const WITH_TEST_MEGAREPO_CONFIGS_CLIENT: &str = "with-test-megarepo-configs-client";
pub const CRYPTO_PATH_REGEX_ARG: &str = "crypto-path-regex";
pub const DERIVE_REMOTELY: &str = "derive-remotely";
pub const DERIVE_REMOTELY_TIER: &str = "derive-remotely-tier";

pub const ACL_FILE: &str = "acl-file";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgType {
    /// Options related to mononoke config
    Config,
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
    /// Adds options for scrubbing blobstores
    Scrub,
    /// Adds --source-repo-id/repo-name and --target-repo-id/repo-name options.
    /// Necessary for crossrepo operations
    /// Only visible if Repo group is visible.
    SourceAndTargetRepos,
    /// Adds just --source-repo-id/repo-name, for blobimport into a megarepo
    /// Only visible if Repo group is visible.
    SourceRepo,
    /// Adds --shutdown-grace-period and --shutdown-timeout for graceful shutdown.
    ShutdownTimeouts,
    /// Adds --scuba-dataset for scuba logging.
    ScubaLogging,
    /// Adds --disabled-hooks for disabling hooks.
    DisableHooks,
    /// Adds --fb303-thrift-port for stats and profiling
    Fb303,
    /// Adds --enable-mcrouter to use McRouter to talk to Memcache for places that support it,
    /// which can boot faster in dev binaries.
    McRouter,
    /// Adds arguments related to Scribe logging.
    Scribe,
    /// Adds options related to rendezvous
    RendezVous,
    /// Adds options related to derivation
    Derivation,
    /// Adds options related to acls
    Acls,
}

// Arguments that are enabled by default for MononokeAppBuilder
const DEFAULT_ARG_TYPES: &[ArgType] = &[
    ArgType::Blobstore,
    ArgType::Cachelib,
    ArgType::Config,
    ArgType::Logging,
    ArgType::Mysql,
    ArgType::Repo,
    ArgType::Runtime,
    ArgType::Tunables,
    ArgType::RendezVous,
    ArgType::Derivation,
    ArgType::Acls,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepoRequirement {
    // The command will execute for exactly one repo at a time.
    ExactlyOne,
    // The command requires atleast one repo.
    AtLeastOne,
}

/// Build clap App with appropriate default settings.
pub struct MononokeAppBuilder {
    /// The app name.
    name: String,

    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    hide_advanced_args: bool,

    /// Flag determinig if the command supports getting repos dynamically at runtime in
    /// addition to repos being provided through CLI before execution.
    dynamic_repos: bool,

    /// Whether to require the user select a repo if the option is present.
    repo_required: Option<RepoRequirement>,

    /// Which groups of arguments are enabled for this app
    arg_types: HashSet<ArgType>,

    /// This app is special admin tool, needs to run with specific PutBehaviour
    special_put_behaviour: Option<PutBehaviour>,

    /// Cachelib default settings, as shown in usage
    cachelib_settings: CachelibSettings,

    /// Whether to default to readonly storage or not
    readonly_storage_default: ReadOnlyStorage,

    /// Whether to default to attempting to compress to cachelib for large objects
    blobstore_cachelib_attempt_zstd_default: bool,

    /// Whether to default to limit blobstore read QPS
    blobstore_read_qps_default: Option<NonZeroU32>,

    /// The default Scuba dataset for this app, if any.
    default_scuba_dataset: Option<String>,

    // Whether to default to scrubbing when using a multiplexed blobstore
    scrub_action_default: Option<ScrubAction>,

    // Whether to allow a grace period before reporting a key missing in a store for recent keys
    scrub_grace_secs_default: Option<u64>,

    // Whether to report missing keys in write only blobstores as a scrub action when scrubbing
    scrub_action_on_missing_write_only_default: Option<SrubWriteOnly>,

    // Whether to set a default for how long to peek back at the multiplex queue when scrubbing
    scrub_queue_peek_bound_secs_default: Option<u64>,

    /// Additional filter for customising logging
    slog_filter_fn: Option<fn(&Record) -> bool>,
}

/// Things we want to live for the lifetime of the mononoke binary
#[derive(Default)]
pub struct MononokeAppData {
    pub cachelib_settings: CachelibSettings,
    pub repo_required: Option<RepoRequirement>,
    pub global_mysql_connection_pool: SharedConnectionPool,
    pub sqlblob_mysql_connection_pool: SharedConnectionPool,
    pub default_scuba_dataset: Option<String>,
    pub slog_filter_fn: Option<fn(&Record) -> bool>,
}

// Result of MononokeAppBuilder::build() which has clap plus the MononokeApp data
pub struct MononokeClapApp<'a, 'b> {
    clap: App<'a, 'b>,
    app_data: MononokeAppData,
    arg_types: HashSet<ArgType>,
}

impl<'a, 'b> MononokeClapApp<'a, 'b> {
    pub fn about<S: Into<&'b str>>(self, about: S) -> Self {
        Self {
            clap: self.clap.about(about),
            ..self
        }
    }

    pub fn subcommand(self, subcmd: App<'a, 'b>) -> Self {
        Self {
            clap: self.clap.subcommand(subcmd),
            ..self
        }
    }

    pub fn arg<A: Into<Arg<'a, 'b>>>(mut self, a: A) -> Self {
        self.clap.p.add_arg(a.into());
        self
    }

    pub fn args_from_usage(self, usage: &'a str) -> Self {
        Self {
            clap: self.clap.args_from_usage(usage),
            ..self
        }
    }

    pub fn group(self, group: ArgGroup<'a>) -> Self {
        Self {
            clap: self.clap.group(group),
            ..self
        }
    }

    pub fn get_matches(self, fb: FacebookInit) -> Result<MononokeMatches<'a>, Error> {
        MononokeMatches::new(fb, self.clap.get_matches(), self.app_data, self.arg_types)
    }

    pub fn get_matches_from<I, T>(
        self,
        fb: FacebookInit,
        itr: I,
    ) -> Result<MononokeMatches<'a>, Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        MononokeMatches::new(
            fb,
            self.clap.get_matches_from(itr),
            self.app_data,
            self.arg_types,
        )
    }
}

impl MononokeAppBuilder {
    /// Start building a new Mononoke app.  This adds the standard Mononoke args.  Use the `build`
    /// method to get a `clap::App` that you can then customize further.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hide_advanced_args: false,
            repo_required: None,
            arg_types: DEFAULT_ARG_TYPES.iter().cloned().collect(),
            special_put_behaviour: None,
            cachelib_settings: CachelibSettings::default(),
            readonly_storage_default: ReadOnlyStorage(false),
            blobstore_cachelib_attempt_zstd_default: true,
            blobstore_read_qps_default: None,
            default_scuba_dataset: None,
            scrub_action_default: None,
            scrub_grace_secs_default: None,
            scrub_action_on_missing_write_only_default: None,
            scrub_queue_peek_bound_secs_default: None,
            slog_filter_fn: None,
            dynamic_repos: false,
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
    pub fn with_repo_required(mut self, requirement: RepoRequirement) -> Self {
        self.arg_types.insert(ArgType::Repo);
        self.repo_required = Some(requirement);
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

    /// This command can get repos through CLI OR
    /// at runtime dynamically through sharded execution.
    pub fn with_dynamic_repos(mut self) -> Self {
        self.dynamic_repos = true;
        self
    }

    /// This command has arguments for McRouter
    pub fn with_mcrouter_args(mut self) -> Self {
        self.arg_types.insert(ArgType::McRouter);
        self
    }

    pub fn with_scribe_args(mut self) -> Self {
        self.arg_types.insert(ArgType::Scribe);
        self
    }

    pub fn with_default_scuba_dataset(mut self, default: impl Into<String>) -> Self {
        self.default_scuba_dataset = Some(default.into());
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

    /// This command has a special default readonly storage setting
    pub fn with_readonly_storage_default(mut self, v: ReadOnlyStorage) -> Self {
        self.readonly_storage_default = v;
        self
    }

    /// This command has a special default blobstore_cachelib_attempt_zstd setting
    pub fn with_blobstore_cachelib_attempt_zstd_default(mut self, d: bool) -> Self {
        self.blobstore_cachelib_attempt_zstd_default = d;
        self
    }

    /// This command has a special default blobstore_read_qps default setting
    pub fn with_blobstore_read_qps_default(mut self, d: Option<NonZeroU32>) -> Self {
        self.blobstore_read_qps_default = d;
        self
    }

    /// This command has different cachelib defaults, show them in --help
    pub fn with_cachelib_settings(mut self, cachelib_settings: CachelibSettings) -> Self {
        self.cachelib_settings = cachelib_settings;
        self
    }

    /// This command has a special scrub_action default setting
    pub fn with_scrub_action_default(mut self, d: Option<ScrubAction>) -> Self {
        self.scrub_action_default = d;
        self
    }

    /// This command has a special grace period for recent keys when scrubbing
    pub fn with_scrub_grace_secs_default(mut self, d: Option<u64>) -> Self {
        self.scrub_grace_secs_default = d;
        self
    }

    /// This command has a special handling of write only stores when scrubbing
    pub fn with_scrub_action_on_missing_write_only_default(
        mut self,
        d: Option<SrubWriteOnly>,
    ) -> Self {
        self.scrub_action_on_missing_write_only_default = d;
        self
    }

    /// Enables custom logging filter
    pub fn with_slog_filter(mut self, slog_filter_fn: fn(&Record) -> bool) -> Self {
        self.slog_filter_fn = Some(slog_filter_fn);
        self
    }

    /// Build a MononokeClapApp around a `clap::App` for this Mononoke app, which can then be customized further.
    pub fn build<'a, 'b>(self) -> MononokeClapApp<'a, 'b> {
        let mut app = App::new(self.name.clone());

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

            let mut repo_id_arg = Arg::with_name(REPO_ID)
                .long(REPO_ID)
                // This is an old form that some consumers use
                .alias("repo_id")
                .value_name("ID")
                .help("numeric ID of repository")
                .conflicts_with_all(repo_conflicts);

            let mut repo_name_arg = Arg::with_name(REPO_NAME)
                .long(REPO_NAME)
                .value_name("NAME")
                .help("Name of repository")
                .conflicts_with_all(repo_conflicts);

            let sharded_service_name_arg = Arg::with_name(SHARDED_SERVICE_NAME)
                .long(SHARDED_SERVICE_NAME)
                .value_name("NAME")
                .multiple(false)
                .help("The name of SM service to be used when the command needs to be executed in a sharded setting")
                .conflicts_with_all(repo_conflicts);

            let group_args = if self.dynamic_repos {
                vec![REPO_ID, REPO_NAME, SHARDED_SERVICE_NAME]
            } else {
                vec![REPO_ID, REPO_NAME]
            };
            let mut repo_group = ArgGroup::with_name("repo")
                .args(&group_args)
                .required(self.repo_required.is_some());

            if self.repo_required == Some(RepoRequirement::AtLeastOne) {
                repo_id_arg = repo_id_arg.multiple(true).number_of_values(1);
                repo_name_arg = repo_name_arg.multiple(true).number_of_values(1);
                repo_group = repo_group.multiple(true)
            }
            app = if self.dynamic_repos {
                app.arg(sharded_service_name_arg)
                    .arg(repo_id_arg)
                    .arg(repo_name_arg)
                    .group(repo_group)
            } else {
                app.arg(repo_id_arg).arg(repo_name_arg).group(repo_group)
            };

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
            app = self.add_blobstore_args(app);
        }
        if self.arg_types.contains(&ArgType::Cachelib) {
            app = add_cachelib_args(app, self.hide_advanced_args, self.cachelib_settings.clone());
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
            app = add_scuba_logging_args(app, self.default_scuba_dataset.is_some());
        }
        if self.arg_types.contains(&ArgType::DisableHooks) {
            app = add_disabled_hooks_args(app);
        }
        if self.arg_types.contains(&ArgType::Fb303) {
            app = add_fb303_args(app);
        }
        if self.arg_types.contains(&ArgType::McRouter) {
            app = add_mcrouter_args(app);
        }
        if self.arg_types.contains(&ArgType::Scribe) {
            app = add_scribe_logging_args(app);
        }
        if self.arg_types.contains(&ArgType::RendezVous) {
            app = add_rendezvous_args(app);
        }
        if self.arg_types.contains(&ArgType::Derivation) {
            app = add_derivation_args(app);
        }
        if self.arg_types.contains(&ArgType::Acls) {
            app = add_acls_args(app);
        }

        app = add_megarepo_svc_args(app);

        MononokeClapApp {
            clap: app,
            app_data: MononokeAppData {
                cachelib_settings: self.cachelib_settings,
                repo_required: self.repo_required,
                global_mysql_connection_pool: SharedConnectionPool::new(),
                sqlblob_mysql_connection_pool: SharedConnectionPool::new(),
                default_scuba_dataset: self.default_scuba_dataset,
                slog_filter_fn: self.slog_filter_fn,
            },
            arg_types: self.arg_types,
        }
    }

    fn add_blobstore_args<'a, 'b>(&self, app: App<'a, 'b>) -> App<'a, 'b> {
        let mut put_arg = Arg::with_name(BLOBSTORE_PUT_BEHAVIOUR_ARG)
            .long(BLOBSTORE_PUT_BEHAVIOUR_ARG)
            .takes_value(true)
            .required(false)
            .help("Desired blobstore behaviour when a put is made to an existing key.");

        if let Some(special_put_behaviour) = self.special_put_behaviour {
            put_arg = put_arg.default_value(special_put_behaviour.into());
        } else {
            // Add the default here so that it shows in --help
            put_arg = put_arg.default_value(DEFAULT_PUT_BEHAVIOUR.into());
        }

        let mut read_qps_arg = Arg::with_name(READ_QPS_ARG)
            .long(READ_QPS_ARG)
            .takes_value(true)
            .required(false)
            .help("Read QPS limit to ThrottledBlob");

        if let Some(default) = self.blobstore_read_qps_default {
            // Lazy static is nicer to LeakSanitizer than Box::leak
            static QPS_FORMATTED: OnceCell<String> = OnceCell::new();
            // clap needs &'static str
            read_qps_arg =
                read_qps_arg.default_value(QPS_FORMATTED.get_or_init(|| format!("{}", default)));
        }

        let app = app.arg(
           read_qps_arg
        )
        .arg(
            Arg::with_name(WRITE_QPS_ARG)
                .long(WRITE_QPS_ARG)
                .takes_value(true)
                .required(false)
                .help("Write QPS limit to ThrottledBlob"),
        )
        .arg(
            Arg::with_name(WRITE_BYTES_ARG)
                .long(WRITE_BYTES_ARG)
                .takes_value(true)
                .required(false)
                .help("Write Bytes/s limit to ThrottledBlob"),
        )
        .arg(
            Arg::with_name(READ_BYTES_ARG)
                .long(READ_BYTES_ARG)
                .takes_value(true)
                .required(false)
                .help("Read Bytes/s limit to ThrottledBlob"),
        )
        .arg(
            Arg::with_name(READ_BURST_BYTES_ARG)
                .long(READ_BURST_BYTES_ARG)
                .takes_value(true)
                .required(false)
                .help("Maximum burst bytes/s limit to ThrottledBlob.  Blobs larger than this will error rather than throttle due to consuming too much quota."),
        )
        .arg(
            Arg::with_name(WRITE_BURST_BYTES_ARG)
                .long(WRITE_BURST_BYTES_ARG)
                .takes_value(true)
                .required(false)
                .help("Maximum burst bytes/s limit to ThrottledBlob.  Blobs larger than this will error rather than throttle due to consuming too much quota."),
        )
        .arg(
            Arg::with_name(BLOBSTORE_BYTES_MIN_THROTTLE_ARG)
                .long(BLOBSTORE_BYTES_MIN_THROTTLE_ARG)
                .takes_value(true)
                .required(false)
                .help("Minimum number of bytes ThrottledBlob can count"),
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
                .possible_values(BOOL_VALUES)
                .help("Allows one to override config to enable/disable zstd compression on write via packblob"),
        )
        .arg(
            Arg::with_name(WRITE_ZSTD_LEVEL_ARG)
                .long(WRITE_ZSTD_LEVEL_ARG)
                .takes_value(true)
                .required(false)
                .requires(WRITE_ZSTD_ARG)
                .help("Override the zstd compression leve used for writes via packblob."),
        )
        .arg(
            Arg::with_name(CACHELIB_ATTEMPT_ZSTD_ARG)
                .long(CACHELIB_ATTEMPT_ZSTD_ARG)
                .takes_value(true)
                .possible_values(BOOL_VALUES)
                .required(false)
                .default_value(bool_as_str(self.blobstore_cachelib_attempt_zstd_default))
                .help("Whether to attempt zstd compression when blobstore is putting things into cachelib over threshold size."),
        )
        .arg(
          put_arg
        )
        .arg(
            Arg::with_name(WITH_READONLY_STORAGE_ARG)
                .long(WITH_READONLY_STORAGE_ARG)
                .takes_value(true)
                .possible_values(BOOL_VALUES)
                .default_value(bool_as_str(self.readonly_storage_default.0))
                .help("Error on any attempts to write to storage if set to true"),
        )
        .arg(
            Arg::with_name(PUT_MEAN_DELAY_SECS_ARG)
                .long(PUT_MEAN_DELAY_SECS_ARG)
                .takes_value(true)
                .value_name(PUT_MEAN_DELAY_SECS_ARG)
                .requires(PUT_STDDEV_DELAY_SECS_ARG)
                .help("Mean value of additional delay for blobstore put calls"),
        )
        .arg(
            Arg::with_name(PUT_STDDEV_DELAY_SECS_ARG)
                .long(PUT_STDDEV_DELAY_SECS_ARG)
                .takes_value(true)
                .value_name(PUT_STDDEV_DELAY_SECS_ARG)
                .requires(PUT_MEAN_DELAY_SECS_ARG)
                .help("Stddev value of additional delay for blobstore put calls"),
        )
        .arg(
            Arg::with_name(GET_MEAN_DELAY_SECS_ARG)
                .long(GET_MEAN_DELAY_SECS_ARG)
                .takes_value(true)
                .value_name(GET_MEAN_DELAY_SECS_ARG)
                .requires(GET_STDDEV_DELAY_SECS_ARG)
                .help("Mean value of additional delay for blobstore get calls"),
        )
        .arg(
            Arg::with_name(GET_STDDEV_DELAY_SECS_ARG)
                .long(GET_STDDEV_DELAY_SECS_ARG)
                .takes_value(true)
                .value_name(GET_STDDEV_DELAY_SECS_ARG)
                .requires(GET_MEAN_DELAY_SECS_ARG)
                .help("Stddev value of additional delay for blobstore get calls"),
        );

        #[cfg(fbcode_build)]
        let app = blobstore_factory::ManifoldOptions::add_args(app);

        if self.arg_types.contains(&ArgType::Scrub) {
            let mut scrub_action_arg = Arg::with_name(BLOBSTORE_SCRUB_ACTION_ARG)
                .long(BLOBSTORE_SCRUB_ACTION_ARG)
                .takes_value(true)
                .required(false)
                .possible_values(ScrubAction::VARIANTS)
                .help("Enable ScrubBlobstore with the given action. Checks for keys missing from stores. In ReportOnly mode this logs only, otherwise it performs a copy to the missing stores.");
            if let Some(default) = self.scrub_action_default {
                scrub_action_arg = scrub_action_arg.default_value(default.into());
            }
            let mut scrub_grace_arg = Arg::with_name(BLOBSTORE_SCRUB_GRACE_ARG)
                .long(BLOBSTORE_SCRUB_GRACE_ARG)
                .takes_value(true)
                .required(false)
                .help("Number of seconds grace to give for key to arrive in multiple blobstores or the healer queue when scrubbing");
            if let Some(default) = self.scrub_grace_secs_default {
                static FORMATTED: OnceCell<String> = OnceCell::new(); // Lazy static is nicer to LeakSanitizer than Box::leak
                scrub_grace_arg =
                    scrub_grace_arg.default_value(FORMATTED.get_or_init(|| format!("{}", default)));
            }
            let mut scrub_queue_peek_bound_arg = Arg::with_name(
                BLOBSTORE_SCRUB_QUEUE_PEEK_BOUND_ARG,
            )
            .long(BLOBSTORE_SCRUB_QUEUE_PEEK_BOUND_ARG)
            .takes_value(true)
            .required(false)
            .requires(BLOBSTORE_SCRUB_ACTION_ARG)
            .help("Number of seconds within which we consider it worth checking the healer queue.");
            if let Some(default) = self.scrub_queue_peek_bound_secs_default {
                static FORMATTED: OnceCell<String> = OnceCell::new(); // Lazy static is nicer to LeakSanitizer than Box::leak
                scrub_queue_peek_bound_arg = scrub_queue_peek_bound_arg
                    .default_value(FORMATTED.get_or_init(|| format!("{}", default)));
            };
            let mut scrub_action_on_missing_write_only_arg =
                Arg::with_name(BLOBSTORE_SCRUB_WRITE_ONLY_MISSING_ARG)
                    .long(BLOBSTORE_SCRUB_WRITE_ONLY_MISSING_ARG)
                    .takes_value(true)
                    .required(false)
                    .possible_values(SrubWriteOnly::VARIANTS)
                    .help("Whether to allow missing values from write only stores when scrubbing");
            if let Some(default) = self.scrub_action_on_missing_write_only_default {
                scrub_action_on_missing_write_only_arg =
                    scrub_action_on_missing_write_only_arg.default_value(default.into());
            }
            app.arg(scrub_action_arg)
                .arg(scrub_grace_arg)
                .arg(scrub_action_on_missing_write_only_arg)
                .arg(scrub_queue_peek_bound_arg)
        } else {
            app
        }
    }
}

fn add_tunables_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(TUNABLES_CONFIG)
            .long(TUNABLES_CONFIG)
            .takes_value(true)
            .help("Tunables dynamic config path in Configerator"),
    )
    .arg(
        Arg::with_name(TUNABLES_LOCAL_PATH)
            .long(TUNABLES_LOCAL_PATH)
            .conflicts_with(TUNABLES_CONFIG)
            .takes_value(true)
            .help("Tunables static config local path"),
    )
    .arg(
        Arg::with_name(DISABLE_TUNABLES)
            .long(DISABLE_TUNABLES)
            .conflicts_with_all(&[TUNABLES_CONFIG, TUNABLES_LOCAL_PATH])
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
        Arg::with_name(LOGVIEW_CATEGORY)
            .long(LOGVIEW_CATEGORY)
            .takes_value(true)
            .help("logview category to log to. Logview is not used if not set"),
    )
    .arg(
        Arg::with_name(LOGVIEW_ADDITIONAL_LEVEL_FILTER)
            .long(LOGVIEW_ADDITIONAL_LEVEL_FILTER)
            .takes_value(true)
            .possible_values(&slog::LOG_LEVEL_NAMES)
            .requires(LOGVIEW_CATEGORY)
            .help("logview level to filter. If logview category is not set then this is ignored. \
             Note that this level is applied AFTER --log-level/--debug was applied, so it doesn't make sense to set this parameter to a lower level \
             than --log-level"),
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
            .possible_values(&slog::LOG_LEVEL_NAMES)
            .conflicts_with("debug"),
    )
    .arg(
        Arg::with_name(LOG_INCLUDE_TAG)
            .long(LOG_INCLUDE_TAG)
            .short("l")
            .help("include only log messages with these slog::Record::tags()/log::Record::targets")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1),
    )
    .arg(
        Arg::with_name(LOG_EXCLUDE_TAG)
            .long(LOG_EXCLUDE_TAG)
            .short("L")
            .help("exclude log messages with these slog::Record::tags()/log::Record::targets")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1),
    )
    .arg(
        Arg::with_name(WITH_DYNAMIC_OBSERVABILITY)
            .long(WITH_DYNAMIC_OBSERVABILITY)
            .help(
                "whether to instantiate ObservabilityContext::Dynamic,\
                 which reads logging levels from configerator. Overwrites\
                 --log-level or --debug",
            )
            .takes_value(true)
            .possible_values(&["true", "false"])
            .default_value("false"),
    )
}

fn add_mysql_options_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(MYSQL_MASTER_ONLY)
            .long(MYSQL_MASTER_ONLY)
            .help("Connect to MySQL master only")
            .takes_value(false),
    )
    // All the defaults for Mysql connection pool are derived from sql_ext::facebook::mysql
    // https://fburl.com/diffusion/n5isd68j
    // last synced on 17/12/2020
    .arg(
        Arg::with_name(MYSQL_POOL_LIMIT)
            .long(MYSQL_POOL_LIMIT)
            .help("Size of the connection pool")
            .takes_value(true)
            .default_value("10000"),
    )
    .arg(
        Arg::with_name(MYSQL_POOL_PER_KEY_LIMIT)
            .long(MYSQL_POOL_PER_KEY_LIMIT)
            .help("Mysql connection pool per key limit")
            .takes_value(true)
            .default_value("100"),
    )
    .arg(
        Arg::with_name(MYSQL_POOL_THREADS_NUM)
            .long(MYSQL_POOL_THREADS_NUM)
            .help("Number of threads in Mysql connection pool, i.e. number of real pools")
            .takes_value(true)
            .default_value("10"),
    )
    .arg(
        Arg::with_name(MYSQL_POOL_AGE_TIMEOUT)
            .long(MYSQL_POOL_AGE_TIMEOUT)
            .help("Mysql connection pool age timeout in millisecs")
            .takes_value(true)
            .default_value("60000"),
    )
    .arg(
        Arg::with_name(MYSQL_POOL_IDLE_TIMEOUT)
            .long(MYSQL_POOL_IDLE_TIMEOUT)
            .help("Mysql connection pool idle timeout in millisecs")
            .takes_value(true)
            .default_value("4000"),
    )
    // SQLBlob wants more aggressive timeouts, and does not benefit from sharing a pool with other users.
    .arg(
        Arg::with_name(MYSQL_SQLBLOB_POOL_LIMIT)
            .long(MYSQL_SQLBLOB_POOL_LIMIT)
            .help("Size of the connection pool")
            .takes_value(true)
            .default_value("10000"),
    )
    .arg(
        Arg::with_name(MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT)
            .long(MYSQL_SQLBLOB_POOL_PER_KEY_LIMIT)
            .help("Mysql connection pool per key limit")
            .takes_value(true)
            .default_value("100"),
    )
    .arg(
        Arg::with_name(MYSQL_SQLBLOB_POOL_THREADS_NUM)
            .long(MYSQL_SQLBLOB_POOL_THREADS_NUM)
            .help("Number of threads in Mysql connection pool, i.e. number of real pools")
            .takes_value(true)
            .default_value("10"),
    )
    .arg(
        Arg::with_name(MYSQL_SQLBLOB_POOL_AGE_TIMEOUT)
            .long(MYSQL_SQLBLOB_POOL_AGE_TIMEOUT)
            .help("Mysql connection pool age timeout in millisecs")
            .takes_value(true)
            .default_value("60000"),
    )
    .arg(
        Arg::with_name(MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT)
            .long(MYSQL_SQLBLOB_POOL_IDLE_TIMEOUT)
            .help("Mysql connection pool idle timeout in millisecs")
            .takes_value(true)
            .default_value("1000"),
    )
    .arg(
        Arg::with_name(MYSQL_CONN_OPEN_TIMEOUT)
            .long(MYSQL_CONN_OPEN_TIMEOUT)
            .help("Mysql connection open timeout in millisecs")
            .takes_value(true)
            .default_value("3000"),
    )
    .arg(
        Arg::with_name(MYSQL_MAX_QUERY_TIME)
            .long(MYSQL_MAX_QUERY_TIME)
            .help("Mysql query time limit in millisecs")
            .takes_value(true)
            .default_value("10000"),
    )
}

pub fn bool_as_str(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

pub(crate) const BOOL_VALUES: &[&str] = &["false", "true"];

fn add_mcrouter_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ENABLE_MCROUTER)
            .long(ENABLE_MCROUTER)
            .help("Use local McRouter for rate limits")
            .takes_value(false),
    )
}

fn add_fb303_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
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

fn add_scuba_logging_args<'a, 'b>(app: App<'a, 'b>, has_default: bool) -> App<'a, 'b> {
    let mut app = app
        .arg(
            Arg::with_name(SCUBA_DATASET_ARG)
                .long(SCUBA_DATASET_ARG)
                .takes_value(true)
                .help("The name of the scuba dataset to log to"),
        )
        .arg(
            Arg::with_name(SCUBA_LOG_FILE_ARG)
                .long(SCUBA_LOG_FILE_ARG)
                .takes_value(true)
                .help("A log file to write JSON Scuba logs to (primarily useful in testing)"),
        )
        .arg(
            Arg::with_name(WARM_BOOKMARK_CACHE_SCUBA_DATASET_ARG)
                .long(WARM_BOOKMARK_CACHE_SCUBA_DATASET_ARG)
                .takes_value(true)
                .help(
                    "Special dataset to be used by warm bookmark cache. \
                If a binary doesn't use warm bookmark cache then this parameter is ignored",
                ),
        );

    if has_default {
        app = app.arg(
            Arg::with_name(NO_DEFAULT_SCUBA_DATASET_ARG)
                .long(NO_DEFAULT_SCUBA_DATASET_ARG)
                .takes_value(false)
                .help("Do not to the default scuba dataset for this app"),
        )
    }

    app
}

fn add_scribe_logging_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(SCRIBE_LOGGING_DIRECTORY)
            .long(SCRIBE_LOGGING_DIRECTORY)
            .takes_value(true)
            .help("Filesystem directory where to log all scribe writes"),
    )
}

fn add_rendezvous_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(RENDEZVOUS_FREE_CONNECTIONS)
            .long(RENDEZVOUS_FREE_CONNECTIONS)
            .takes_value(true)
            .default_value("5")
            .help("How many concurrent connections to allow before batching kicks in"),
    )
}

fn add_megarepo_svc_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(WITH_TEST_MEGAREPO_CONFIGS_CLIENT)
            .long(WITH_TEST_MEGAREPO_CONFIGS_CLIENT)
            .help(
                "whether to instantiate test-style MononokeMegarepoConfigs. \
                     Prod-style instance reads/writes from/to configerator and \
                     requires fb environment to work properly.",
            )
            .takes_value(true)
            .possible_values(&["true", "false"])
            .default_value("false"),
    )
}

fn add_derivation_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(DERIVE_REMOTELY)
            .long(DERIVE_REMOTELY)
            .help("Derive data remotely using default service"),
    )
    .arg(
        Arg::with_name(DERIVE_REMOTELY_TIER)
            .long(DERIVE_REMOTELY_TIER)
            .takes_value(true)
            .value_name("SMC")
            .help("Specify smc tier for derived data service"),
    )
}

fn add_acls_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ACL_FILE)
            .long(ACL_FILE)
            .takes_value(true)
            .value_name("PATH")
            .help("Specify a file containing ACLs"),
    )
}
