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
extern crate tokio_core;
extern crate tracing_fb303;

extern crate blobrepo;
extern crate bookmarks;
extern crate mercurial_types;
extern crate metaconfig;
extern crate ready_state;
extern crate repo_listener;

mod errors;
mod monitoring;

use std::io;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{App, ArgMatches};
use failure::SlogKVError;
use futures::Future;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;

use blobrepo::BlobRepo;
use mercurial_types::RepositoryId;
use metaconfig::RepoConfigs;

use errors::*;

// Exit the whole process if any of the threads fails to catch a panic
fn setup_panic_hook() {
    let original_hook = panic::take_hook();

    panic::set_hook(Box::new(move |info| {
        original_hook(info);
        std::process::exit(1);
    }));
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("mononoke server")
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
        "#,
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
            let logview_drain = LogViewDrain::new("errorlog_mononoke");
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

fn get_config<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    // TODO: This needs to cope with blob repos, too
    let crpath = PathBuf::from(matches.value_of("crpath").unwrap());
    let config_repo = BlobRepo::new_rocksdb(
        logger.new(o!["repo" => "Config repo"]),
        &crpath,
        RepositoryId::new(0),
    )?;

    let changesetid = match matches.value_of("crbook") {
        Some(book) => {
            let book = bookmarks::Bookmark::new(book).expect("book must be ascii");
            println!("Looking for bookmark {:?}", book);
            config_repo
                .get_bookmark(&book)
                .wait()?
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

    RepoConfigs::read_config_repo(config_repo, changesetid)
        .from_err()
        .wait()
}

fn main() {
    setup_panic_hook();
    let matches = setup_app().get_matches();
    let root_log = setup_logger(&matches);

    fn run_server<'a>(root_log: &Logger, matches: ArgMatches<'a>) -> Result<!> {
        info!(root_log, "Starting up");

        let stats_aggregation = monitoring::start_stats()?;

        let config = get_config(root_log, &matches)?;
        let cert = matches.value_of("cert").unwrap().to_string();
        let private_key = matches.value_of("private_key").unwrap().to_string();
        let ca_pem = matches.value_of("ca_pem").unwrap().to_string();

        let ssl = secure_utils::SslConfig {
            cert,
            private_key,
            ca_pem,
        };

        let (repo_listeners, ready) = repo_listener::start_repo_listeners(
            config.repos.into_iter(),
            root_log,
            matches
                .value_of("listening-host-port")
                .expect("listening path must be specified"),
            secure_utils::build_tls_acceptor(ssl).expect("failed to build tls acceptor"),
        )?;

        tracing_fb303::register();

        let maybe_thrift = match monitoring::start_thrift_service(&root_log, &matches, ready) {
            None => None,
            Some(handle) => Some(handle?),
        };

        for handle in vec![stats_aggregation]
            .into_iter()
            .chain(maybe_thrift.into_iter())
            .chain(repo_listeners.into_iter())
        {
            let thread_name = handle.thread().name().unwrap_or("unknown").to_owned();
            match handle.join() {
                Ok(_) => panic!("unexpected success"),
                Err(panic) => crit!(
                    root_log,
                    "Thread {} panicked with: {:?}",
                    thread_name,
                    panic
                ),
            }
        }

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
