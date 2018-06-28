// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches};
use slog::{Drain, Level, Logger};

use slog_glog_fmt::default_drain as glog_drain;

use blobrepo::ManifoldArgs;

const CACHE_ARGS: &[(&str, &str)] = &[
    ("blobstore-cache-size", "size of the blobstore cache"),
    ("changesets-cache-size", "size of the changesets cache"),
    ("filenodes-cache-size", "size of the filenodes cache"),
];

pub struct MononokeApp {
    /// Whether to redirect writes to non-production by default. Note that this isn't (yet)
    /// foolproof.
    pub safe_writes: bool,
    /// Whether to hide advanced Manifold configuration from help. Note that the arguments will
    /// still be available, just not displayed in help.
    pub hide_advanced_args: bool,
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

        App::new(name)
            .args_from_usage(
                r#"
                -d, --debug 'print debug output'
                "#,
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
            )
    }
}

pub fn get_logger<'a>(matches: &ArgMatches<'a>) -> Logger {
    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    let drain = glog_drain().filter_level(level).fuse();
    Logger::root(drain, o![])
}

pub fn parse_manifold_args<'a>(
    matches: &ArgMatches<'a>,
    default_cache_size: usize,
) -> ManifoldArgs {
    // The unwraps here are safe because default values have already been provided in mononoke_app
    // above.
    ManifoldArgs {
        bucket: matches.value_of("manifold-bucket").unwrap().to_string(),
        prefix: matches.value_of("manifold-prefix").unwrap().to_string(),
        db_address: matches.value_of("db-address").unwrap().to_string(),
        blobstore_cache_size: get_usize(matches, "blobstore-cache-size", default_cache_size),
        changesets_cache_size: get_usize(matches, "changesets-cache-size", default_cache_size),
        filenodes_cache_size: get_usize(matches, "filenodes-cache-size", default_cache_size),
        io_threads: get_usize(matches, "io-threads", 5),
        max_concurrent_requests_per_io_thread: get_usize(
            matches,
            "max-concurrent-request-per-io-thread",
            5,
        ),
    }
}

pub fn get_usize<'a>(matches: &ArgMatches<'a>, key: &str, default: usize) -> usize {
    matches
        .value_of(key)
        .map(|val| {
            val.parse::<usize>()
                .expect(&format!("{} must be integer", key))
        })
        .unwrap_or(default)
}
