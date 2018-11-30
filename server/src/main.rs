// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate clap;
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate lazy_static;
extern crate openssl;
extern crate secure_utils;
extern crate services;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_logview;
extern crate slog_stats;
extern crate slog_term;
extern crate stats;
extern crate tokio;
extern crate tracing_fb303;

extern crate blobrepo;
extern crate bookmarks;
extern crate cachelib;
extern crate cmdlib;
extern crate context;
extern crate mercurial_types;
extern crate metaconfig;
extern crate panichandler;
extern crate ready_state;
extern crate repo_listener;

mod monitoring;

use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{App, ArgMatches};
use context::CoreContext;
use failure::SlogKVError;
use futures::Future;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;
use tokio::runtime::Runtime;

use blobrepo::BlobRepo;
use mercurial_types::RepositoryId;
use metaconfig::RepoConfigs;

mod errors {
    pub use failure::{Error, Result};
}
use errors::*;

lazy_static! {
    static ref TERMINATE_PROCESS: AtomicBool = AtomicBool::new(false);
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    cmdlib::args::add_cachelib_args(App::new("mononoke server")
        .version("0.0.0")
        .about("serve repos")
        .args_from_usage(
            r#"
            <crpath>      -P, --configrepo_path [PATH]           'path to the config repo in rocksdb form'

            -C, --configrepo_hash [HASH]                         'config repo commit hash'

            <crbook>      -C, --configrepo_book [BOOK]           'config repo bookmark'

                          --listening-host-port <PATH>           'tcp address to listen to in format `host:port`'

            -p, --thrift_port [PORT] 'if provided the thrift server will start on this port'

            <cert>        --cert [PATH]                         'path to a file with certificate'
            <private_key> --private-key [PATH]                  'path to a file with private key'
            <ca_pem>      --ca-pem [PATH]                       'path to a file with CA certificate'

            -d, --debug                                          'print debug level output'
            --myrouter-port=[PORT]                               'port for local myrouter instance'
            "#,
        ),
        false /* hide_advanced_args */
    )
}

fn setup_logger<'a>(matches: &ArgMatches<'a>) -> Logger {
    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    let drain = {
        let drain = {
            // TODO: switch to TermDecorator, which supports color
            let decorator = slog_term::PlainSyncDecorator::new(io::stdout());
            let stderr_drain = GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer);
            // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
            // since logview is sampled anyway.
            let logview_drain = LogViewDrain::new("errorlog_mononoke").ignore_res();
            slog::Duplicate::new(stderr_drain, logview_drain)
        };
        let drain = slog_stats::StatsDrain::new(drain);
        drain.filter_level(level)
    };

    Logger::root(
        drain.fuse(),
        o!(kv_defaults::FacebookKV::new().expect("Failed to initialize logging")),
    )
}

fn get_config<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
    runtime: &mut Runtime,
) -> Result<RepoConfigs> {
    // TODO: This needs to cope with blob repos, too
    let crpath = PathBuf::from(matches.value_of("crpath").unwrap());
    // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
    let ctx = CoreContext::test_mock();
    let config_repo = BlobRepo::new_rocksdb(
        logger.new(o!["repo" => "Config repo"]),
        &crpath,
        RepositoryId::new(0),
    )?;

    let changesetid = match matches.value_of("crbook") {
        Some(book) => {
            let book = bookmarks::Bookmark::new(book).expect("book must be ascii");
            println!("Looking for bookmark {:?}", book);
            runtime
                .block_on(config_repo.get_bookmark(ctx, &book))?
                .expect("bookmark not found")
        }
        None => mercurial_types::nodehash::HgChangesetId::from_str(
            matches
                .value_of("crhash")
                .expect("crhash and crbook are not specified"),
        )?,
    };

    info!(
        logger,
        "Config repository will be read from commit: {}", changesetid
    );

    runtime.block_on(RepoConfigs::read_config_repo(config_repo, changesetid).from_err())
}

fn main() {
    let matches = setup_app().get_matches();
    let root_log = setup_logger(&matches);

    panichandler::set_panichandler(panichandler::Fate::Abort);

    cmdlib::args::init_cachelib(&matches);

    fn run_server<'a>(root_log: &Logger, matches: ArgMatches<'a>) -> Result<!> {
        info!(root_log, "Starting up");

        let stats_aggregation = stats::schedule_stats_aggregation()
            .expect("failed to create stats aggregation scheduler");

        let mut runtime = Runtime::new()?;

        let config = get_config(root_log, &matches, &mut runtime)?;
        let cert = matches.value_of("cert").unwrap().to_string();
        let private_key = matches.value_of("private_key").unwrap().to_string();
        let ca_pem = matches.value_of("ca_pem").unwrap().to_string();

        let ssl = secure_utils::SslConfig {
            cert,
            private_key,
            ca_pem,
        };

        let myrouter_port = match matches.value_of("myrouter-port") {
            Some(port) => Some(
                port.parse::<u16>()
                    .expect("Provided --myrouter-port is not u16"),
            ),
            None => None,
        };

        let (repo_listeners, ready) = repo_listener::create_repo_listeners(
            config.repos.into_iter(),
            myrouter_port,
            root_log,
            matches
                .value_of("listening-host-port")
                .expect("listening path must be specified"),
            secure_utils::build_tls_acceptor(ssl).expect("failed to build tls acceptor"),
            &TERMINATE_PROCESS,
        );

        tracing_fb303::register();

        let sigterm = 15;
        unsafe {
            signal(sigterm, handle_sig_term);
        }

        // Thread with a thrift service is now detached
        monitoring::start_thrift_service(&root_log, &matches, ready);

        runtime.spawn(
            repo_listeners
                .select(stats_aggregation.from_err())
                .map(|((), _)| ())
                .map_err(|(err, _)| panic!("Unexpected error: {:#?}", err)),
        );
        runtime
            .shutdown_on_idle()
            .wait()
            .expect("This runtime should never be idle");

        info!(root_log, "No service to run, shutting down");
        std::process::exit(0);
    }

    match run_server(&root_log, matches) {
        Ok(_) => panic!("unexpected success"),
        Err(e) => {
            crit!(root_log, "Server fatal error"; SlogKVError(e));
            std::process::exit(1);
        }
    }
}

extern "C" {
    fn signal(sig: u32, cb: extern "C" fn(u32)) -> extern "C" fn(u32);
}

extern "C" fn handle_sig_term(_: u32) {
    TERMINATE_PROCESS.store(true, Ordering::Relaxed);
}
