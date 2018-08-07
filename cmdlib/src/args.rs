// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fs;
use std::path::Path;
use std::time::Duration;

use clap::{App, Arg, ArgMatches};
use failure::{Result, ResultExt};
use slog::{Drain, Logger};
use sloggers::Build;
use sloggers::terminal::TerminalLoggerBuilder;
use sloggers::types::{Format, Severity, SourceLocation};

use cachelib;
use slog_glog_fmt::default_drain as glog_drain;

use blobrepo::{BlobRepo, ManifoldArgs};
use mercurial_types::RepositoryId;

const CACHE_ARGS: &[(&str, &str)] = &[
    ("blob-cache-size", "size of the blob cache"),
    ("presence-cache-size", "size of the blob presence cache"),
    ("changesets-cache-size", "size of the changesets cache"),
    ("filenodes-cache-size", "size of the filenodes cache"),
    (
        "idmapping-cache-size",
        "size of the bonsai/hg mapping cache",
    ),
];

pub struct MononokeApp {
    /// Whether to redirect writes to non-production by default. Note that this isn't (yet)
    /// foolproof.
    pub safe_writes: bool,
    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    pub hide_advanced_args: bool,
    /// Whether this tool can deal with local instances (which are very useful for testing).
    pub local_instances: bool,
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
        let hide_advanced_args = self.hide_advanced_args;
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
        let default_log = if self.default_glog { "glog" } else { "compact" };

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
                Arg::with_name("log-style")
                    .short("l")
                    .value_name("STYLE")
                    .possible_values(&["compact", "glog"])
                    .default_value(default_log)
                    .help("log style to use for output")
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
                Arg::with_name("db-address")
                    .long("db-address")
                    .value_name("ADDRESS")
                    .default_value("xdb.mononoke_test_2")
                    .help("database address"),
            )
            .args(&cache_args)
            .arg(
                Arg::with_name("io-threads")
                    .long("io-threads")
                    .value_name("NUM")
                    .default_value("5")
                    .hidden(hide_advanced_args)
                    .help("number of IO threads to use for Manifold")
            )
            .arg(
                Arg::with_name("max-concurrent-request-per-io-thread")
                    .long("max-concurrent-request-per-io-thread")
                    .value_name("NUM")
                    .default_value("5")
                    .hidden(hide_advanced_args)
                    .help("maximum open requests per Manifold IO thread")
            );

        app = add_cachelib_args(app);

        if self.local_instances {
            app = app.arg(
                Arg::with_name("blobstore")
                    .long("blobstore")
                    .value_name("TYPE")
                    .possible_values(&["files", "rocksdb", "manifold"])
                    .default_value("manifold")
                    .help("blobstore type"),
            ).arg(
                Arg::with_name("data-dir")
                    .long("data-dir")
                    .value_name("DIR")
                    .help("local data directory (used for local blobstores)"),
            );
        }

        app
    }
}

pub fn get_logger<'a>(matches: &ArgMatches<'a>) -> Logger {
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
            let drain = glog_drain().filter_level(severity.as_level()).fuse();
            Logger::root(drain, o![])
        }
        "compact" => {
            let mut builder = TerminalLoggerBuilder::new();
            builder.level(severity);
            builder.format(Format::Compact);
            builder.source_location(SourceLocation::None);

            builder.build().unwrap()
        }
        _other => unreachable!("unknown log style"),
    }
}

pub fn get_repo_id<'a>(matches: &ArgMatches<'a>) -> RepositoryId {
    let repo_id = matches
        .value_of("repo-id")
        .unwrap()
        .parse::<u32>()
        .expect("expected repository ID to be a u32");
    RepositoryId::new(repo_id as i32)
}

/// Create a new `BlobRepo` -- for local instances, expect its contents to be empty.
#[inline]
pub fn create_blobrepo<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> BlobRepo {
    open_blobrepo_internal(logger, matches, true)
}

/// Open an existing `BlobRepo` -- for local instances, expect contents to already be there.
#[inline]
pub fn open_blobrepo<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> BlobRepo {
    open_blobrepo_internal(logger, matches, false)
}

pub fn setup_blobrepo_dir<P: AsRef<Path>>(data_dir: P, create: bool) -> Result<()> {
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

pub fn add_cachelib_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(Arg::from_usage(
            "--cache-size-gb [SIZE] 'size of the cachelib cache, in GiB'",
    ))
    .arg(Arg::from_usage(
            "--max-process-size [SIZE] 'process size at which cachelib will shrink, in GiB'"
    ))
    .arg(Arg::from_usage(
            "--min-process-size [SIZE] 'process size at which cachelib will grow back to cache-size-gb, in GiB'"
    ))
}

pub fn init_cachelib<'a>(matches: &ArgMatches<'a>) {
    let cache_size = matches
        .value_of("cache-size-gb")
        .unwrap_or("20")
        .parse::<usize>()
        .unwrap() * 1024 * 1024 * 1024;
    let max_process_size_gib = matches
        .value_of("max-process-size")
        .map(str::parse::<usize>)
        // This is a fixed-point calculation. The process can grow to 1.2 times the size of the
        // cache before the cache shrinks, but cachelib is inconsistent and wants cache size in
        // bytes but process sizes in GiB
        .unwrap_or(Ok(cache_size * 12 / (10 * 1024 * 1024 * 1024)))
        .unwrap() as u32;
    let min_process_size_gib = matches
        .value_of("min-process-size")
        .map(str::parse::<usize>)
        // This is a fixed-point calculation. The process can fall to 0.8 times the size of the
        // cache before the cache will regrow to target size, but cachelib is inconsistent
        // and wants cache size in bytes but process sizes in GiB
        .unwrap_or(Ok(cache_size * 8 / (10 * 1024 * 1024 * 1024)))
        .unwrap() as u32;

    cachelib::init_cache_once(
        cachelib::LruCacheConfig::new(cache_size)
            .set_shrinker(cachelib::ShrinkMonitor {
                shrinker_type: cachelib::ShrinkMonitorType::ResidentSize {
                    max_process_size_gib: max_process_size_gib,
                    min_process_size_gib: min_process_size_gib,
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
            })
            .set_pool_rebalance(cachelib::PoolRebalanceConfig {
                interval: Duration::new(10, 0),
                strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                    // A small increase in hit ratio is desired
                    diff_ratio: 0.05,
                    min_retained_slabs: 1,
                    // Objects newer than 30 seconds old might be about to become interesting
                    min_tail_age: Duration::new(30, 0),
                    ignore_untouched_slabs: false,
                },
            }),
    ).unwrap();

    cachelib::init_cacheadmin("mononoke").unwrap();

    cachelib::get_or_create_pool(
        "blobstore-blobs",
        get_usize(matches, "blob-cache-size", 100_000_000),
    ).unwrap();
    cachelib::get_or_create_pool(
        "blobstore-presence",
        get_usize(matches, "presence-cache-size", 100_000_000),
    ).unwrap();
    cachelib::get_or_create_pool(
        "changesets",
        get_usize(matches, "changesets-cache-size", 100_000_000),
    ).unwrap();
    cachelib::get_or_create_pool(
        "filenodes",
        get_usize(matches, "filenodes-cache-size", 100_000_000),
    ).unwrap();
    cachelib::get_or_create_pool(
        "bonsai_hg_mapping",
        get_usize(matches, "idmapping-cache-size", 100_000_000),
    ).unwrap();
}

fn open_blobrepo_internal<'a>(logger: &Logger, matches: &ArgMatches<'a>, create: bool) -> BlobRepo {
    let repo_id = get_repo_id(matches);

    match matches.value_of("blobstore") {
        Some("files") => {
            let data_dir = matches
                .value_of("data-dir")
                .expect("local data directory must be specified");
            let data_dir = Path::new(data_dir)
                .canonicalize()
                .expect("Failed to read local directory path");
            setup_blobrepo_dir(&data_dir, create).expect("Setting up file blobrepo failed");

            BlobRepo::new_files(
                logger.new(o!["BlobRepo:Files" => data_dir.to_string_lossy().into_owned()]),
                &data_dir,
                repo_id,
            ).expect("failed to create file blobrepo")
        }
        Some("rocksdb") => {
            let data_dir = matches
                .value_of("data-dir")
                .expect("local directory must be specified");
            let data_dir = Path::new(data_dir)
                .canonicalize()
                .expect("Failed to read local directory path");
            setup_blobrepo_dir(&data_dir, create).expect("Setting up rocksdb blobrepo failed");

            BlobRepo::new_rocksdb(
                logger.new(o!["BlobRepo:Rocksdb" => data_dir.to_string_lossy().into_owned()]),
                &data_dir,
                repo_id,
            ).expect("failed to create rocksdb blobrepo")
        }
        None | Some("manifold") => {
            let manifold_args = parse_manifold_args(&matches);

            BlobRepo::new_manifold(
                logger.new(o!["BlobRepo:TestManifold" => manifold_args.bucket.clone()]),
                &manifold_args,
                repo_id,
            ).expect("failed to create manifold blobrepo")
        }
        Some(bad) => panic!("unexpected blobstore type: {}", bad),
    }
}

pub fn parse_manifold_args<'a>(matches: &ArgMatches<'a>) -> ManifoldArgs {
    // The unwraps here are safe because default values have already been provided in mononoke_app
    // above.
    ManifoldArgs {
        bucket: matches.value_of("manifold-bucket").unwrap().to_string(),
        prefix: matches.value_of("manifold-prefix").unwrap().to_string(),
        db_address: matches.value_of("db-address").unwrap().to_string(),
        io_threads: get_usize(matches, "io-threads", 5),
        max_concurrent_requests_per_io_thread: get_usize(
            matches,
            "max-concurrent-request-per-io-thread",
            5,
        ),
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
