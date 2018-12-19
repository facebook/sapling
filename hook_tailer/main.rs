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
extern crate cloned;
extern crate cmdlib;
extern crate context;
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

use bookmarks::Bookmark;
use clap::{App, ArgMatches};
use context::CoreContext;
use failure::Error;
use failure::Result;
use futures::Stream;
use futures::future::Future;
use futures::stream::repeat;
use futures_ext::{BoxFuture, FutureExt};
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution};
use manifold::{ManifoldHttpClient, RequestContext};
use mercurial_types::HgNodeHash;
use metaconfig::RepoConfigs;
use mononoke_types::RepositoryId;
use repo_client::open_blobrepo;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;
use slog_scuba::ScubaDrain;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tailer::Tailer;
use tokio_timer::sleep;

pub struct HookResults {
    file_hooks_results: Vec<(FileHookExecutionID, HookExecution)>,
    cs_hooks_result: Vec<(ChangesetHookExecutionID, HookExecution)>,
}

fn main() -> Result<()> {
    panichandler::set_panichandler(panichandler::Fate::Abort);

    let matches = setup_app().get_matches();
    let repo_name = matches.value_of("repo_name").unwrap();
    let logger = setup_logger(&matches, repo_name.to_string());
    info!(logger, "Hook tailer is starting");
    let configs = get_config(&matches)?;
    let bookmark_name = matches.value_of("bookmark").unwrap();
    let bookmark = Bookmark::new(bookmark_name).unwrap();
    let err: Error = ErrorKind::NoSuchRepo(repo_name.into()).into();
    let config = configs.repos.get(repo_name).ok_or(err)?;

    cmdlib::args::init_cachelib(&matches);

    let myrouter_port = match matches.value_of("myrouter-port") {
        Some(port) => Some(
            port.parse::<u16>()
                .expect("Provided --myrouter-port is not u16"),
        ),
        None => None,
    };

    let blobrepo = open_blobrepo(
        logger.clone(),
        config.repotype.clone(),
        RepositoryId::new(config.repoid),
        myrouter_port,
    )?;

    let rc = RequestContext {
        bucket_name: "mononoke_prod".into(),
        api_key: "mononoke_prod-key".into(),
        timeout_msec: 10000,
    };

    let id = "ManifoldBlob";

    let manifold_client = ManifoldHttpClient::new(id, rc)?;

    // TODO(T37478150, luk) This is not a test case, will be fixed in later diffs
    let ctx = CoreContext::test_mock();

    let tailer = Tailer::new(
        ctx,
        repo_name.to_string(),
        // TODO (T32873881): Arc<BlobRepo> should become BlobRepo
        Arc::new(blobrepo),
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

    let fut = if matches.is_present("continuous") {
        // Tail new commits and run hooks on them
        let logger = logger.clone();
        fut.then(|_| {
            repeat(()).for_each(move |()| {
                let fut = tailer.run();
                process_hook_results(fut, logger.clone()).and_then(|()| {
                    sleep(Duration::new(10, 0))
                        .map_err(|err| format_err!("Tokio timer error {:?}", err))
                })
            })
        }).left_future()
    } else {
        let limit = cmdlib::args::get_u64(&matches, "limit", 1000);
        let logger = logger.clone();
        fut.then(move |_| {
            let fut = tailer.run_with_limit(limit);
            process_hook_results(fut, logger)
        }).right_future()
    };

    tokio::run(fut.map(|_| ()).map_err(move |err| {
        error!(logger, "Failed to run tailer {:?}", err);
    }));

    Ok(())
}

fn process_hook_results(
    fut: BoxFuture<Vec<HookResults>, Error>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    fut.map(move |res| {
        let mut file_hooks_stat = HookExecutionStat::new();
        let mut cs_hooks_stat = HookExecutionStat::new();

        res.into_iter().for_each(|hook_results| {
            let HookResults {
                file_hooks_results,
                cs_hooks_result,
            } = hook_results;
            debug!(logger, "==== File hooks results ====");
            file_hooks_results.into_iter().for_each(|(exec_id, exec)| {
                file_hooks_stat.record_hook_execution(&exec);

                debug!(
                    logger,
                    "changeset:{} hook_name:{} path:{} result:{:?}",
                    exec_id.cs_id,
                    exec_id.hook_name,
                    exec_id.file.path,
                    exec
                );
            });
            debug!(logger, "==== Changeset hooks results ====");
            cs_hooks_result.into_iter().for_each(|(exec_id, exec)| {
                cs_hooks_stat.record_hook_execution(&exec);
                debug!(
                    logger,
                    "changeset:{} hook_name:{} result:{:?}", exec_id.cs_id, exec_id.hook_name, exec
                );
            });
        });

        info!(logger, "==== File hooks stat: {} ====", file_hooks_stat);
        info!(logger, "==== Changeset hooks stat: {} ====", cs_hooks_stat);

        ()
    }).boxify()
}

struct HookExecutionStat {
    accepted: usize,
    rejected: usize,
}

impl HookExecutionStat {
    pub fn new() -> Self {
        Self {
            accepted: 0,
            rejected: 0,
        }
    }

    pub fn record_hook_execution(&mut self, exec: &hooks::HookExecution) {
        match exec {
            hooks::HookExecution::Accepted => {
                self.accepted += 1;
            }
            hooks::HookExecution::Rejected(_) => {
                self.rejected += 1;
            }
        };
    }
}

impl fmt::Display for HookExecutionStat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "accepted: {}, rejected: {}",
            self.accepted, self.rejected
        )
    }
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    cmdlib::args::add_cachelib_args(App::new("mononoke hook server")
        .version("0.0.0")
        .about("run hooks against repo")
        .args_from_usage(
            r#"
            <cpath>      -P, --config_path [PATH]           'path to the config files'

            <bookmark>    -B, --bookmark [BOOK]                  'bookmark to tail'
                           --poll-interval                       'the poll interval in seconds'

            <repo_name>   -R, --repo_name [REPO_NAME]            'the name of the repo to run hooks for'

                          --init_revision [INIT_REVISION]        'the initial revision to start at'

            --continuous                                         'continuously run hooks on new commits'
            --limit=[LIMIT]                                      'limit number of commits to process (non-continuous only). Default: 1000'
            -d, --debug                                          'print debug level output'
            -p, --myrouter-port=[PORT]                           'port for local myrouter instance'
        "#,
    ),
        false /* hide_advanced_args */
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

fn get_config<'a>(matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    let cpath = PathBuf::from(matches.value_of("cpath").unwrap());
    RepoConfigs::read_configs(cpath)
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No such repo '{}'", _0)] NoSuchRepo(String),
}
