// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    cmp::min,
    collections::HashMap,
    fs,
    num::NonZeroUsize,
    path::Path,
    time::{Duration, Instant},
};

use clap::{App, Arg, ArgMatches};
use failure_ext::{bail_msg, err_msg, Error, Result, ResultExt};
use futures::{Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use panichandler::{self, Fate};
use scuba_ext::ScubaSampleBuilder;
use slog::{debug, info, o, warn, Drain, Logger};
use sloggers::{
    terminal::{Destination, TerminalLoggerBuilder},
    types::{Format, Severity, SourceLocation},
    Build,
};
use sshrelay::SshEnvVars;
use std::collections::HashSet;
use tracing::TraceContext;
use upload_trace::{manifold_thrift::thrift::RequestContext, UploadTrace};
use uuid::Uuid;

use cachelib;
use slog_glog_fmt::default_drain as glog_drain;

use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, Caching};
use blobstore_factory::Scrubbing;
use changesets::SqlConstructors;
use context::CoreContext;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::{BlobConfig, CommonConfig, MetadataDBConfig, RepoConfig, StorageConfig};
use mononoke_types::RepositoryId;

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
    /// Whether to redirect writes to non-production by default. Note that this isn't (yet)
    /// foolproof.
    pub safe_writes: bool,
    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    pub hide_advanced_args: bool,
    /// Whether to use glog by default.
    pub default_glog: bool,
}

impl MononokeApp {
    /// Create a new Mononoke-based CLI tool. The `safe_writes` option changes some defaults to
    /// avoid production writes. (But it isn't foolproof -- please fix any options that are
    /// missing).
    pub fn build<'a, 'b, S: Into<String>>(self, name: S) -> App<'a, 'b> {
        let default_manifold_prefix = if self.safe_writes {
            "mononoke_test"
        } else {
            ""
        };

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
                    .default_value("0")
                    .help("numeric ID of repository")
            )
            .arg(
                Arg::with_name("mononoke-config-path")
                    .long("mononoke-config-path")
                    .value_name("MONONOKE_CONFIG_PATH")
                    .help("Path to the Mononoke configs")
            )

            // Manifold-specific arguments
            .arg(
                Arg::with_name("manifold-bucket")
                    .long("manifold-bucket")
                    .value_name("BUCKET")
                    .default_value("mononoke_prod")
                    .help("manifold bucket"),
            )
            .arg(
                Arg::with_name("manifold-prefix")
                    .long("manifold-prefix")
                    .value_name("PREFIX")
                    .default_value(default_manifold_prefix)
                    .help("manifold prefix"),
            )
            .arg(
                Arg::with_name("mysql-blobstore-shardmap")
                    .long("mysql-blobstore-shardmap")
                    .value_name("SHARDMAP")
                    .help("mysql blobstore shardmap, if provided mysql is used instead of \
                           manifold"),
            )
            .arg(
                Arg::with_name("mysql-blobstore-shard-num")
                    .long("mysql-blobstore-shard-num")
                    .value_name("SHARD_NUM")
                    .default_value("100")
                    .help("mysql blobstore shard num"),
            )
            .arg(
                Arg::with_name("filenode-shards")
                .long("filenode-shards").value_name("SHARD_COUNT").help("number of shards to spread filenodes across")
            );

        app = add_logger_args(app, self.default_glog);
        app = add_myrouter_args(app);
        app = add_cachelib_args(app, self.hide_advanced_args);

        app
    }
}

pub fn add_logger_args<'a, 'b>(app: App<'a, 'b>, default_glog: bool) -> App<'a, 'b> {
    let default_log = if default_glog { "glog" } else { "compact" };
    app.arg(
        Arg::with_name("log-style")
            .short("l")
            .long("log-style")
            .value_name("STYLE")
            .possible_values(&["compact", "glog"])
            .default_value(default_log)
            .help("log style to use for output"),
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

pub fn get_logger<'a>(matches: &ArgMatches<'a>) -> Logger {
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

    let severity = if matches.is_present("debug") {
        Severity::Debug
    } else {
        Severity::Info
    };

    let log_style = matches
        .value_of("log-style")
        .expect("default style is always specified");
    match log_style {
        "glog" => {
            let glog_drain = glog_drain().filter_level(severity.as_level()).fuse();
            if matches.is_present("fb303-thrift-port") {
                Logger::root(slog_stats::StatsDrain::new(glog_drain), o![])
            } else {
                Logger::root(glog_drain, o![])
            }
        }
        "compact" => {
            let mut builder = TerminalLoggerBuilder::new();
            builder.destination(Destination::Stderr);
            builder.level(severity);
            builder.format(Format::Compact);
            builder.source_location(SourceLocation::None);

            builder.build().unwrap()
        }
        _other => unreachable!("unknown log style"),
    }
}

pub fn get_core_context<'a>(matches: &ArgMatches<'a>) -> CoreContext {
    let session_uuid = Uuid::new_v4();
    let trace = TraceContext::new(session_uuid, Instant::now());
    let logger = get_logger(&matches);

    CoreContext::new(
        session_uuid,
        logger.clone(),
        ScubaSampleBuilder::with_discard(),
        None,
        trace.clone(),
        None,
        SshEnvVars::default(),
        // TODO(stash): use load limiting?
        None,
    )
}

pub fn upload_and_show_trace(ctx: CoreContext) -> impl Future<Item = (), Error = !> {
    if !ctx.trace().is_enabled() {
        debug!(ctx.logger(), "Trace is disabled");
        return Ok(()).into_future().left_future();
    }

    let rc = RequestContext {
        bucketName: "mononoke_prod".into(),
        apiKey: "".into(),
        ..Default::default()
    };

    ctx.trace()
        .upload_to_manifold(rc)
        .then(move |upload_res| {
            match upload_res {
                Err(err) => debug!(ctx.logger(), "Failed to upload trace: {:#?}", err),
                Ok(()) => debug!(
                    ctx.logger(),
                    "Trace taken: https://our.intern.facebook.com/intern/mononoke/trace/{}",
                    ctx.trace().id()
                ),
            }
            Ok(())
        })
        .right_future()
}

pub fn get_repo_id<'a>(matches: &ArgMatches<'a>) -> Result<RepositoryId> {
    let repo_id = matches
        .value_of("repo-id")
        .ok_or(err_msg("Missing repo-id"))?
        .parse::<u32>()
        .map_err(|_| err_msg("Couldn't parse repo-id as u32"))?;
    Ok(RepositoryId::new(repo_id as i32))
}

pub fn open_sql<T>(matches: &ArgMatches<'_>) -> BoxFuture<T, Error>
where
    T: SqlConstructors,
{
    let name = T::LABEL;

    let (_, config) = try_boxfuture!(get_config(matches));

    match config.storage_config.dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            T::with_sqlite_path(path.join(name)).into_future().boxify()
        }
        MetadataDBConfig::Mysql { db_address, .. } if name != "filenodes" => {
            T::with_xdb(db_address, parse_myrouter_port(matches))
        }
        MetadataDBConfig::Mysql { .. } => Err(err_msg(
            "Use SqlFilenodes::with_sharded_myrouter for filenodes",
        ))
        .into_future()
        .boxify(),
    }
}

/// Create a new `BlobRepo` -- for local instances, expect its contents to be empty.
#[inline]
pub fn create_repo<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    open_repo_internal(
        logger,
        matches,
        true,
        parse_caching(matches),
        Scrubbing::Disabled,
    )
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
#[inline]
pub fn open_repo<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    open_repo_internal(
        logger,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Disabled,
    )
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
/// If there are multiple backing blobstores, open them in scrub mode, where we check that
/// the blobstore contents all match.
#[inline]
pub fn open_scrub_repo<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    open_repo_internal(
        logger,
        matches,
        false,
        parse_caching(matches),
        Scrubbing::Enabled,
    )
}

pub fn setup_repo_dir<P: AsRef<Path>>(data_dir: P, create: bool) -> Result<()> {
    let data_dir = data_dir.as_ref();

    if !data_dir.is_dir() {
        bail_msg!("{:?} does not exist or is not a directory", data_dir);
    }

    for subdir in &["blobs"] {
        let subdir = data_dir.join(subdir);

        if subdir.exists() && !subdir.is_dir() {
            bail_msg!("{:?} already exists and is not a directory", subdir);
        }

        if create {
            if subdir.exists() {
                let content: Vec<_> = subdir.read_dir()?.collect();
                if !content.is_empty() {
                    bail_msg!(
                        "{:?} already exists and is not empty: {:?}",
                        subdir,
                        content
                    );
                }
            } else {
                fs::create_dir(&subdir)
                    .with_context(|_| format!("failed to create subdirectory {:?}", subdir))?;
            }
        }
    }
    Ok(())
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
    .args(&cache_args)
}

pub struct CachelibSettings {
    pub cache_size: usize,
    pub max_process_size_gib: Option<u32>,
    pub min_process_size_gib: Option<u32>,
    pub use_tupperware_shrinker: bool,
    pub presence_cache_size: Option<usize>,
    pub changesets_cache_size: Option<usize>,
    pub filenodes_cache_size: Option<usize>,
    pub idmapping_cache_size: Option<usize>,
    pub with_content_sha1_cache: bool,
    pub content_sha1_cache_size: Option<usize>,
    pub blob_cache_size: Option<usize>,
}

impl Default for CachelibSettings {
    fn default() -> Self {
        Self {
            cache_size: 20 * 1024 * 1024 * 1024,
            max_process_size_gib: None,
            min_process_size_gib: None,
            use_tupperware_shrinker: false,
            presence_cache_size: None,
            changesets_cache_size: None,
            filenodes_cache_size: None,
            idmapping_cache_size: None,
            with_content_sha1_cache: false,
            content_sha1_cache_size: None,
            blob_cache_size: None,
        }
    }
}

fn parse_caching<'a>(matches: &ArgMatches<'a>) -> Caching {
    if matches.is_present("skip-caching") {
        Caching::Disabled
    } else {
        Caching::Enabled
    }
}

pub fn init_cachelib<'a>(matches: &ArgMatches<'a>) -> Caching {
    let caching = parse_caching(matches);

    if let Caching::Enabled = caching {
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

        init_cachelib_from_settings(settings).unwrap();
    }

    caching
}

pub fn init_cachelib_from_settings(settings: CachelibSettings) -> Result<()> {
    // Millions of lookups per second
    let lock_power = 10;
    // Assume 200 bytes average cache item size and compute bucketsPower
    let expected_item_size_bytes = 200;
    let cache_size_bytes = settings.cache_size;
    let item_count = cache_size_bytes / expected_item_size_bytes;

    // Because `bucket_count` is a power of 2, bucket_count.trailing_zeros() is log2(bucket_count)
    let bucket_count = item_count
        .checked_next_power_of_two()
        .ok_or_else(|| err_msg("Cache has too many objects to fit a `usize`?!?"))?;
    let buckets_power = min(bucket_count.trailing_zeros() + 1 as u32, 32);

    let mut cache_config = cachelib::LruCacheConfig::new(cache_size_bytes)
        .set_pool_rebalance(cachelib::PoolRebalanceConfig {
            interval: Duration::new(300, 0),
            strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                // A small increase in hit ratio is desired
                diff_ratio: 0.05,
                min_retained_slabs: 1,
                // Objects newer than 30 seconds old might be about to become interesting
                min_tail_age: Duration::new(30, 0),
                ignore_untouched_slabs: false,
            },
        })
        .set_access_config(buckets_power, lock_power);

    if settings.use_tupperware_shrinker {
        if settings.max_process_size_gib.is_some() || settings.min_process_size_gib.is_some() {
            return Err(err_msg(
                "Can't use both Tupperware shrinker and manually configured shrinker",
            ));
        }
        cache_config = cache_config.set_tupperware_shrinker();
    } else {
        match (settings.max_process_size_gib, settings.min_process_size_gib) {
            (None, None) => (),
            (Some(_), None) | (None, Some(_)) => {
                return Err(err_msg(
                    "If setting process size limits, must set both max and min",
                ));
            }
            (Some(max), Some(min)) => {
                cache_config = cache_config.set_shrinker(cachelib::ShrinkMonitor {
                    shrinker_type: cachelib::ShrinkMonitorType::ResidentSize {
                        max_process_size_gib: max,
                        min_process_size_gib: min,
                    },
                    interval: Duration::new(10, 0),
                    max_resize_per_iteration_percent: 25,
                    max_removed_percent: 50,
                    strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                        // A small increase in hit ratio is desired
                        diff_ratio: 0.05,
                        min_retained_slabs: 1,
                        // Objects newer than 30 seconds old might be about to become interesting
                        min_tail_age: Duration::new(30, 0),
                        ignore_untouched_slabs: false,
                    },
                });
            }
        };
    }

    cachelib::init_cache_once(cache_config)?;
    cachelib::init_cacheadmin("mononoke")?;

    // Give each cache 5% of the available space, bar the blob cache which gets everything left
    // over. We can adjust this with data.
    let available_space = cachelib::get_available_space()?;
    cachelib::get_or_create_volatile_pool(
        "blobstore-presence",
        settings.presence_cache_size.unwrap_or(available_space / 20),
    )?;

    cachelib::get_or_create_volatile_pool(
        "changesets",
        settings
            .changesets_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "filenodes",
        settings
            .filenodes_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "bonsai_hg_mapping",
        settings
            .idmapping_cache_size
            .unwrap_or(available_space / 20),
    )?;

    if settings.with_content_sha1_cache {
        cachelib::get_or_create_volatile_pool(
            "content-sha1",
            settings
                .content_sha1_cache_size
                .unwrap_or(available_space / 20),
        )?;
    }

    cachelib::get_or_create_volatile_pool(
        "blobstore-blobs",
        settings
            .blob_cache_size
            .unwrap_or(cachelib::get_available_space()?),
    )?;

    Ok(())
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
    let mut repo_config: Vec<_> = configs
        .repos
        .into_iter()
        .filter(|(_, config)| RepositoryId::new(config.repoid) == repo_id)
        .collect();
    if repo_config.is_empty() {
        Err(err_msg(format!("unknown repoid {:?}", repo_id)))
    } else if repo_config.len() > 1 {
        Err(err_msg(format!(
            "repoid {:?} defined multiple times",
            repo_id
        )))
    } else {
        Ok(repo_config.pop().unwrap())
    }
}

fn open_repo_internal<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
    create: bool,
    caching: Caching,
    scrub: Scrubbing,
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

    repo_id
        .into_future()
        .and_then(move |repo_id| {
            open_blobrepo(
                config.storage_config,
                repo_id,
                myrouter_port,
                caching,
                config.bookmarks_cache_ttl,
                config.censoring,
                common_config.scuba_censored_table,
                config.filestore,
            )
        })
        .boxify()
}

pub fn parse_blobstore_args<'a>(matches: &ArgMatches<'a>) -> BlobConfig {
    // The unwraps here are safe because default values have already been provided in mononoke_app
    // above.
    match matches.value_of("mysql-blobstore-shardmap") {
        Some(shardmap) => BlobConfig::Mysql {
            shard_map: shardmap.to_string(),
            shard_num: matches
                .value_of("mysql-blobstore-shard-num")
                .unwrap()
                .parse::<usize>()
                .ok()
                .and_then(NonZeroUsize::new)
                .expect("Provided mysql-blobstore-shard-num must be int larger than 0"),
        },
        None => BlobConfig::Manifold {
            bucket: matches.value_of("manifold-bucket").unwrap().to_string(),
            prefix: matches.value_of("manifold-prefix").unwrap().to_string(),
        },
    }
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
    matches
        .value_of(key)
        .map(|val| {
            val.parse::<u64>()
                .expect(&format!("{} must be integer", key))
        })
        .unwrap_or(default)
}

#[inline]
pub fn get_u64_opt<'a>(matches: &ArgMatches<'a>, key: &str) -> Option<u64> {
    matches.value_of(key).map(|val| {
        val.parse::<u64>()
            .expect(&format!("{} must be integer", key))
    })
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
