// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

#[cfg(test)]
extern crate async_unit;
extern crate blobrepo;
extern crate blobstore;
extern crate bookmarks;
extern crate bytes;
extern crate clap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate hooks;
extern crate manifold;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_types;
extern crate panichandler;
extern crate repo_client;
extern crate revset;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_kvfilter;
extern crate slog_logview;
extern crate slog_scuba;
extern crate slog_stats;
extern crate slog_term;
extern crate tokio;
extern crate tokio_timer;

pub mod tailer;

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use clap::{App, ArgMatches};
use failure::Error;
use failure::Result;
use futures::future::{loop_fn, Future, Loop};
use futures_ext::{BoxFuture, FutureExt};
use manifold::{ManifoldHttpClient, RequestContext};
use mercurial_types::{HgNodeHash, RepositoryId};
use metaconfig::RepoConfigs;
use repo_client::MononokeRepo;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;
use slog_scuba::ScubaDrain;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tailer::Tailer;
use tokio_timer::sleep;

fn main() -> Result<()> {
    panichandler::set_panichandler(panichandler::Fate::Abort);

    let matches = setup_app().get_matches();
    let repo_name = matches.value_of("repo_name").unwrap();
    let logger = setup_logger(&matches, repo_name.to_string());
    info!(logger, "Hook tailer is starting");
    let configs = get_config(&logger, &matches)?;
    let bookmark_name = matches.value_of("bookmark").unwrap();
    let bookmark = Bookmark::new(bookmark_name).unwrap();
    let err: Error = ErrorKind::NoSuchRepo(repo_name.into()).into();
    let config = configs.repos.get(repo_name).ok_or(err)?;

    let mononoke_repo = MononokeRepo::new(
        logger.clone(),
        &config.repotype,
        RepositoryId::new(config.repoid),
    )?;

    let rc = RequestContext {
        bucket_name: "mononoke_prod".into(),
        api_key: "mononoke_prod-key".into(),
        timeout_msec: 10000,
    };

    let id = "ManifoldBlob";

    let manifold_client = ManifoldHttpClient::new(id, rc)?;

    let tailer = Tailer::new(
        repo_name.to_string(),
        // TODO (T32873881): Arc<BlobRepo> should become BlobRepo
        Arc::new(mononoke_repo.blobrepo().clone()),
        config.clone(),
        bookmark,
        manifold_client.clone(),
        logger.clone(),
    )?;

    let fut = match matches.value_of("init_revision") {
        Some(init_rev) => {
            info!(
                logger.clone(),
                "Initial revision specified as argument {}",
                init_rev
            );
            let hash = HgNodeHash::from_str(init_rev)?;
            let bytes = hash.as_bytes().into();
            manifold_client
                .write(tailer.get_last_rev_key(), bytes)
                .map(|_| ())
                .boxify()
        }
        None => futures::future::ok(()).boxify(),
    };

    let fut = fut.then(|_| create_poller(tailer, logger));

    tokio::run(fut);

    Ok(())
}

fn create_poller(tailer: Tailer, logger: Logger) -> BoxFuture<(), ()> {
    let logger2 = logger.clone();
    let logger3 = logger.clone();
    let lf = loop_fn(tailer, move |tailer| {
        let logger4 = logger2.clone();
        tailer
            .run()
            .map(move |res| {
                res.into_iter().for_each(|(v_files, v_cs)| {
                    info!(logger4, "==== File hooks results ====");
                    v_files.into_iter().for_each(|(exec_id, exec)| {
                        info!(
                            logger4,
                            "changeset:{} hook_name:{} path:{} result:{:?}",
                            exec_id.cs_id,
                            exec_id.hook_name,
                            exec_id.path,
                            exec
                        );
                    });
                    info!(logger4, "==== Changeset hooks results ====");
                    v_cs.into_iter().for_each(|(exec_id, exec)| {
                        info!(
                            logger4,
                            "changeset:{} hook_name:{} result:{:?}",
                            exec_id.cs_id,
                            exec_id.hook_name,
                            exec
                        );
                    });
                });
                ()
            })
            .and_then(|()| {
                sleep(Duration::new(10, 0))
                    .map_err(|err| format_err!("Tokio timer error {:?}", err))
            })
            .and_then(move |()| Ok(Loop::Continue(tailer)))
    });
    lf.map_err(move |err| {
        error!(logger3, "Failed to run tailer {:?}", err);
        ()
    }).boxify()
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("mononoke hook server")
        .version("0.0.0")
        .about("run hooks against repo")
        .args_from_usage(
            r#"
            <crpath>      -P, --configrepo_path [PATH]           'path to the config repo in rocksdb form'

            -C, --configrepo_hash [HASH]                         'config repo commit hash'

            <crbook>      -C, --configrepo_book [BOOK]           'config repo bookmark'

            <bookmark>    -B, --bookmark [BOOK]                  'bookmark to tail'
                           --poll-interval                       'the poll interval in seconds'

            <repo_name>   -R, --repo_name [REPO_NAME]            'the name of the repo to run hooks for'

                          --init_revision [INIT_REVISION]        'the initial revision to start at'

            -d, --debug                                          'print debug level output'
        "#,
        )
}

fn setup_logger<'a>(matches: &ArgMatches<'a>, repo_name: String) -> Logger {
    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    let drain = {
        let drain = {
            let decorator = slog_term::PlainSyncDecorator::new(io::stdout());
            let stderr_drain = GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer);
            let logview_drain = LogViewDrain::new("mononoke_hook_tailer_log");
            let scuba_drain = ScubaDrain::new("mononoke_hook_tailer");
            let drain = slog::Duplicate::new(stderr_drain, logview_drain);
            slog::Duplicate::new(scuba_drain, drain)
        };
        let drain = slog_stats::StatsDrain::new(drain);
        drain.filter_level(level)
    };

    Logger::root(
        drain.fuse(),
        o!("repo" => repo_name,
        kv_defaults::FacebookKV::new().expect("Failed to initialize logging")),
    )
}

fn get_config<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    let crpath = PathBuf::from(matches.value_of("crpath").unwrap());
    let config_repo = BlobRepo::new_rocksdb(
        logger.new(o!["repo" => "Config repo"]),
        &crpath,
        RepositoryId::new(0),
    )?;

    let changesetid = match matches.value_of("crbook") {
        Some(book) => {
            let book = bookmarks::Bookmark::new(book).expect("book must be ascii");
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

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No such repo '{}'", _0)] NoSuchRepo(String),
}
