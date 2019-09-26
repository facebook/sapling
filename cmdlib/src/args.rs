// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{collections::HashMap, path::Path, sync::Arc};

use clap::{App, Arg, ArgMatches};
use cloned::cloned;
use failure_ext::{err_msg, Error, Result};
use fbinit::FacebookInit;
use futures::{Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use panichandler::{self, Fate};
use slog::{debug, info, o, warn, Drain, Level, Logger};
use std::collections::HashSet;

use slog_glog_fmt::default_drain as glog_drain;

use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, Caching};
use blobstore_factory::Scrubbing;
use changesets::SqlConstructors;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::{BlobConfig, CommonConfig, Redaction, RepoConfig, StorageConfig};
use mononoke_types::RepositoryId;

use crate::helpers::{
    init_cachelib_from_settings, open_sql_with_config_and_myrouter_port, setup_repo_dir,
    CachelibSettings,
};
use crate::log;

const CACHE_ARGS: &[(&str, &str)] = &[
    ("blob-cache-size", "override size of the blob cache"),
    (
        "presence-cache-size",
        "override size of the blob presence cache",
    ),
    (
        "changesets-cache-size",
        "override size of the changesets cache",
    ),
    (
        "filenodes-cache-size",
        "override size of the filenodes cache",
    ),
    (
        "idmapping-cache-size",
        "override size of the bonsai/hg mapping cache",
    ),
    (
        "content-sha1-cache-size",
        "override size of the content SHA1 cache",
    ),
];

pub struct MononokeApp {
    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    pub hide_advanced_args: bool,
}

impl MononokeApp {
    pub fn build<'a, 'b, S: Into<String>>(self, name: S) -> App<'a, 'b> {
        let name = name.into();

        let mut app = App::new(name)
            .args_from_usage(
                r#"
                -d, --debug 'print debug output'
                "#,
            )
            .arg(
                Arg::with_name("repo-id")
                    .long("repo-id")
                    // This is an old form that some consumers use
                    .alias("repo_id")
                    .value_name("ID")
                    .help("numeric ID of repository"),
            )
            .arg(
                Arg::with_name("repo-name")
                    .long("repo-name")
                    .value_name("NAME")
                    .help("Name of repository"),
            )
            .arg(
                Arg::with_name("mononoke-config-path")
                    .long("mononoke-config-path")
                    .value_name("MONONOKE_CONFIG_PATH")
                    .help("Path to the Mononoke configs"),
            );

        app = add_logger_args(app);
        app = add_myrouter_args(app);
        app = add_cachelib_args(app, self.hide_advanced_args);

        app
    }
}

pub fn add_logger_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("log-style")
            .short("l")
            .long("log-style")
            .value_name("STYLE")
            .help("DEPRECATED - log style to use for output (doesn't do anything)")
            .hidden(true),
    )
    .arg(
        Arg::with_name("panic-fate")
            .long("panic-fate")
            .value_name("PANIC_FATE")
            .possible_values(&["continue", "exit", "abort"])
            .default_value("abort")
            .help("fate of the process when a panic happens"),
    )
}

pub fn init_logging<'a>(matches: &ArgMatches<'a>) -> Logger {
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
        Level::Info
    };

    let glog_drain = Arc::new(glog_drain());

    // NOTE: We pass an unfitlered Logger to init_stdlog_once. That's because we do the filtering
    // at the stdlog level there.
    let stdlog_level = log::init_stdlog_once(Logger::root(glog_drain.clone(), o![]), stdlog_env);

    let glog_drain = glog_drain.filter_level(level).fuse();

    let logger = if matches.is_present("fb303-thrift-port") {
        Logger::root(slog_stats::StatsDrain::new(glog_drain), o![])
    } else {
        Logger::root(glog_drain, o![])
    };

    debug!(
        logger,
        "enabled stdlog with level: {:?} (set {} to configure)", stdlog_level, stdlog_env
    );

    logger
}

pub fn get_repo_id_and_name_from_values(
    repo_name: Option<&str>,
    repo_id: Option<&str>,
    configs: RepoConfigs,
) -> Result<(RepositoryId, String)> {
    match (repo_name, repo_id) {
        (Some(_), Some(_)) => Err(err_msg("both repo-name and repo-id parameters set")),
        (None, None) => Err(err_msg("neither repo-name nor repo-id parameter set")),
        (None, Some(repo_id)) => {
            let repo_id = repo_id
                .parse::<u32>()
                .map_err(|_| err_msg("Couldn't parse repo-id as u32"))?;
            let mut repo_config: Vec<_> = configs
                .repos
                .into_iter()
                .filter(|(_, repo_config)| repo_config.repoid == repo_id as i32)
                .collect();
            if repo_config.is_empty() {
                Err(err_msg(format!("unknown config for repo-id {:?}", repo_id)))
            } else if repo_config.len() > 1 {
                Err(err_msg(format!(
                    "multiple configs defined for repo-id {:?}",
                    repo_id
                )))
            } else {
                let (repo_name, repo_config) = repo_config.pop().unwrap();
                Ok((RepositoryId::new(repo_config.repoid), repo_name))
            }
        }
        (Some(repo_name), None) => {
            let mut repo_config: Vec<_> = configs
                .repos
                .into_iter()
                .filter(|(name, _)| name == repo_name)
                .collect();
            if repo_config.is_empty() {
                Err(err_msg(format!("unknown repo-name {:?}", repo_name)))
            } else if repo_config.len() > 1 {
                Err(err_msg(format!(
                    "multiple configs defined for repo-name {:?}",
                    repo_name
                )))
            } else {
                let (repo_name, repo_config) = repo_config.pop().unwrap();
                Ok((RepositoryId::new(repo_config.repoid), repo_name))
            }
        }
    }
}

pub fn get_repo_id<'a>(matches: &ArgMatches<'a>) -> Result<RepositoryId> {
    let repo_name = matches.value_of("repo-name");
    let repo_id = matches.value_of("repo-id");
    let configs = read_configs(matches)?;
    let (repo_id, _) = get_repo_id_and_name_from_values(repo_name, repo_id, configs)?;
    Ok(repo_id)
}

pub fn get_repo_name<'a>(matches: &ArgMatches<'a>) -> Result<String> {
    let repo_name = matches.value_of("repo-name");
    let repo_id = matches.value_of("repo-id");
    let configs = read_configs(matches)?;
    let (_, repo_name) = get_repo_id_and_name_from_values(repo_name, repo_id, configs)?;
    Ok(repo_name)
}

pub fn open_sql<T>(matches: &ArgMatches<'_>) -> BoxFuture<T, Error>
where
    T: SqlConstructors,
{
    let (_, config) = try_boxfuture!(get_config(matches));
    let maybe_myrouter_port = parse_myrouter_port(matches);
    open_sql_with_config_and_myrouter_port(config, maybe_myrouter_port)
}

/// Create a new `BlobRepo` -- for local instances, expect its contents to be empty.
#[inline]
pub fn create_repo<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
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
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
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
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
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
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
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
pub fn open_scrub_repo<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
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

pub fn add_cachelib_args<'a, 'b>(app: App<'a, 'b>, hide_advanced_args: bool) -> App<'a, 'b> {
    let cache_args: Vec<_> = CACHE_ARGS
        .iter()
        .map(|(flag, help)| {
            // XXX figure out a way to get default values in here -- note that .default_value
            // takes a &'a str, so we may need to have MononokeApp own it or similar.
            Arg::with_name(flag)
                .long(flag)
                .value_name("SIZE")
                .hidden(hide_advanced_args)
                .help(help)
        })
        .collect();

    app.arg(Arg::from_usage(
            "--cache-size-gb [SIZE] 'size of the cachelib cache, in GiB'",
    ))
    .arg(Arg::from_usage(
            "--use-tupperware-shrinker 'Use the Tupperware-aware cache shrinker to avoid OOM'"
    ))
    .arg(Arg::from_usage(
            "--max-process-size [SIZE] 'process size at which cachelib will shrink, in GiB'"
    ))
    .arg(Arg::from_usage(
            "--min-process-size [SIZE] 'process size at which cachelib will grow back to cache-size-gb, in GiB'"
    ))
    .arg(Arg::from_usage(
            "--with-content-sha1-cache  '[Mononoke API Server only] enable content SHA1 cache'"
    ))
    .args_from_usage(
        r#"
        --skip-caching 'do not init cachelib and disable caches (useful for tests)'
        "#,
    )
    .args_from_usage(
        r#"
        --cachelib-only-blobstore 'do not init memcache for blobstore'
        "#,
    )
    .args(&cache_args)
}

pub fn parse_caching<'a>(matches: &ArgMatches<'a>) -> Caching {
    if matches.is_present("skip-caching") {
        Caching::Disabled
    } else if matches.is_present("cachelib-only-blobstore") {
        Caching::CachelibOnlyBlobstore
    } else {
        Caching::Enabled
    }
}

pub fn init_cachelib<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) -> Caching {
    let caching = parse_caching(matches);

    if caching == Caching::Enabled || caching == Caching::CachelibOnlyBlobstore {
        let mut settings = CachelibSettings::default();
        if let Some(cache_size) = matches.value_of("cache-size-gb") {
            settings.cache_size = cache_size.parse::<usize>().unwrap() * 1024 * 1024 * 1024;
        }
        if let Some(max_process_size) = matches.value_of("max-process-size") {
            settings.max_process_size_gib = Some(max_process_size.parse().unwrap());
        }
        if let Some(min_process_size) = matches.value_of("min-process-size") {
            settings.min_process_size_gib = Some(min_process_size.parse().unwrap());
        }
        settings.use_tupperware_shrinker = matches.is_present("use-tupperware-shrinker");
        if let Some(presence_cache_size) = matches.value_of("presence-cache-size") {
            settings.presence_cache_size = Some(presence_cache_size.parse().unwrap());
        }
        if let Some(changesets_cache_size) = matches.value_of("changesets-cache-size") {
            settings.changesets_cache_size = Some(changesets_cache_size.parse().unwrap());
        }
        if let Some(filenodes_cache_size) = matches.value_of("filenodes-cache-size") {
            settings.filenodes_cache_size = Some(filenodes_cache_size.parse().unwrap());
        }
        if let Some(idmapping_cache_size) = matches.value_of("idmapping-cache-size") {
            settings.idmapping_cache_size = Some(idmapping_cache_size.parse().unwrap());
        }
        settings.with_content_sha1_cache = matches.is_present("with-content-sha1-cache");
        if let Some(content_sha1_cache_size) = matches.value_of("content-sha1-cache-size") {
            settings.content_sha1_cache_size = Some(content_sha1_cache_size.parse().unwrap());
        }
        if let Some(blob_cache_size) = matches.value_of("blob-cache-size") {
            settings.blob_cache_size = Some(blob_cache_size.parse().unwrap());
        }

        init_cachelib_from_settings(fb, settings).unwrap();
    }

    caching
}

pub fn add_myrouter_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.args_from_usage(r"--myrouter-port=[PORT]    'port for local myrouter instance'")
}

pub fn add_fb303_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.args_from_usage(r"--fb303-thrift-port=[PORT]    'port for fb303 service'")
}

pub fn add_disabled_hooks_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name("disabled-hooks")
            .long("disable-hook")
            .help("Disable a hook. Pass this argument multiple times to disable multiple hooks.")
            .multiple(true)
            .number_of_values(1)
            .takes_value(true),
    )
}

pub fn read_configs<'a>(matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    let config_path = matches
        .value_of("mononoke-config-path")
        .ok_or(err_msg("mononoke-config-path must be specified"))?;
    RepoConfigs::read_configs(config_path)
}

pub fn read_common_config<'a>(matches: &ArgMatches<'a>) -> Result<CommonConfig> {
    let config_path = matches
        .value_of("mononoke-config-path")
        .ok_or(err_msg("mononoke-config-path must be specified"))?;

    let config_path = Path::new(config_path);
    let common_dir = config_path.join("common");
    let maybe_common_config = if common_dir.is_dir() {
        RepoConfigs::read_common_config(&common_dir)?
    } else {
        None
    };

    let common_config = maybe_common_config.unwrap_or(Default::default());
    Ok(common_config)
}

pub fn read_storage_configs<'a>(
    matches: &ArgMatches<'a>,
) -> Result<HashMap<String, StorageConfig>> {
    let config_path = matches
        .value_of("mononoke-config-path")
        .ok_or(err_msg("mononoke-config-path must be specified"))?;
    RepoConfigs::read_storage_configs(config_path)
}

pub fn get_config<'a>(matches: &ArgMatches<'a>) -> Result<(String, RepoConfig)> {
    let repo_id = get_repo_id(matches)?;
    let configs = read_configs(matches)?;
    configs
        .get_repo_config(repo_id.id())
        .ok_or_else(|| err_msg(format!("unknown repoid {:?}", repo_id)))
        .map(|(name, config)| (name.clone(), config.clone()))
}

fn open_repo_internal<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    create: bool,
    caching: Caching,
    scrub: Scrubbing,
    redaction_override: Option<Redaction>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    let repo_id = get_repo_id(matches);

    let common_config = try_boxfuture!(read_common_config(&matches));

    let (reponame, config) = {
        let (reponame, mut config) = try_boxfuture!(get_config(matches));
        if let Scrubbing::Enabled = scrub {
            config.storage_config.blobstore.set_scrubbed();
        }
        (reponame, config)
    };
    info!(
        logger,
        "using repo \"{}\" repoid {:?}",
        reponame,
        repo_id.as_ref().unwrap()
    );
    match &config.storage_config.blobstore {
        BlobConfig::Files { path } | BlobConfig::Rocks { path } | BlobConfig::Sqlite { path } => {
            setup_repo_dir(path, create).expect("Setting up file blobrepo failed");
        }
        _ => {}
    };

    let myrouter_port = parse_myrouter_port(matches);

    cloned!(logger);
    repo_id
        .into_future()
        .and_then(move |repo_id| {
            open_blobrepo(
                fb,
                config.storage_config,
                repo_id,
                myrouter_port,
                caching,
                config.bookmarks_cache_ttl,
                redaction_override.unwrap_or(config.redaction),
                common_config.scuba_censored_table,
                config.filestore,
                logger,
            )
        })
        .boxify()
}

pub fn parse_myrouter_port<'a>(matches: &ArgMatches<'a>) -> Option<u16> {
    match matches.value_of("myrouter-port") {
        Some(port) => Some(
            port.parse::<u16>()
                .expect("Provided --myrouter-port is not u16"),
        ),
        None => None,
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

pub fn parse_disabled_hooks(matches: &ArgMatches, logger: &Logger) -> HashSet<String> {
    let disabled_hooks: HashSet<String> = matches
        .values_of("disabled-hooks")
        .map(|m| m.collect())
        .unwrap_or(vec![])
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    if disabled_hooks.len() > 0 {
        warn!(
            logger,
            "The following Hooks were disabled: {:?}", disabled_hooks
        );
    }

    disabled_hooks
}
