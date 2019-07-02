// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

#[macro_use]
extern crate failure_ext as failure;

pub mod tailer;

use blobrepo_factory::open_blobrepo;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, Error, Result};
use futures::future::{err, ok, result, Future};
use futures::stream::repeat;
use futures::Stream;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution};
use manifold::{ManifoldHttpClient, RequestContext};
use mercurial_types::{HgChangesetId, HgNodeHash};
use metaconfig_parser::RepoConfigs;
use mononoke_types::RepositoryId;
use slog::{debug, info, o, Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use std::fmt;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
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
    let repo_name = matches.value_of("repo_name").unwrap().to_string();
    let logger = setup_logger(&matches, repo_name.to_string());
    info!(logger, "Hook tailer is starting");
    let configs = get_config(&matches)?;
    let bookmark_name = matches.value_of("bookmark").unwrap();
    let bookmark = BookmarkName::new(bookmark_name).unwrap();
    let err: Error = ErrorKind::NoSuchRepo(repo_name.clone()).into();
    let config = configs.repos.get(&repo_name).ok_or(err)?;
    let init_revision = matches.value_of("init_revision").map(String::from);
    let continuous = matches.is_present("continuous");
    let limit = cmdlib::args::get_u64(&matches, "limit", 1000);
    let changeset = matches.value_of("changeset").map_or(None, |cs| {
        Some(HgChangesetId::from_str(cs).expect("Invalid changesetid"))
    });
    let mut excludes = matches.value_of("exclude").map_or(vec![], |excludes| {
        excludes
            .split(",")
            .map(|cs| HgChangesetId::from_str(cs).expect("Invalid changeset"))
            .collect::<Vec<_>>()
    });

    if let Some(path) = matches.value_of("exclude_file") {
        let changesets = BufReader::new(File::open(path)?)
            .lines()
            .filter_map(|cs_str| {
                cs_str
                    .map_err(Error::from)
                    .and_then(|cs_str| HgChangesetId::from_str(&cs_str))
                    .ok()
            });

        excludes.extend(changesets);
    }

    let caching = cmdlib::args::init_cachelib(&matches);

    let blobrepo = open_blobrepo(
        logger.clone(),
        config.storage_config.clone(),
        RepositoryId::new(config.repoid),
        cmdlib::args::parse_myrouter_port(&matches),
        caching,
        config.bookmarks_cache_ttl,
        config.censoring,
    );

    let rc = RequestContext {
        bucket_name: "mononoke_prod".into(),
        api_key: "mononoke_prod-key".into(),
        timeout_msec: 10000,
    };

    let id = "ManifoldBlob";

    let manifold_client = ManifoldHttpClient::new(id, rc)?;

    let fut = blobrepo.and_then({
        cloned!(logger, config);
        move |blobrepo| {
            // TODO(T37478150, luk) This is not a test case, will be fixed in later diffs
            let ctx = CoreContext::test_mock();

            blobrepo
                .get_hg_bonsai_mapping(ctx.clone(), excludes)
                .and_then({
                    cloned!(manifold_client, logger);
                    move |excl| {
                        result(Tailer::new(
                            ctx,
                            blobrepo,
                            config.clone(),
                            bookmark,
                            manifold_client.clone(),
                            logger.clone(),
                            excl.into_iter().map(|(_, cs)| cs).collect(),
                        ))
                    }
                })
                .and_then({
                    cloned!(manifold_client);
                    move |tail| {
                        let f = match init_revision {
                            Some(init_rev) => {
                                info!(
                                    logger.clone(),
                                    "Initial revision specified as argument {}", init_rev
                                );
                                let hash = try_boxfuture!(HgNodeHash::from_str(&init_rev));
                                let bytes = hash.as_bytes().into();
                                manifold_client
                                    .write(tail.get_last_rev_key(), bytes)
                                    .map(|_| ())
                                    .boxify()
                            }
                            None => futures::future::ok(()).boxify(),
                        };

                        match (continuous, changeset) {
                            (true, _) => {
                                // Tail new commits and run hooks on them
                                let logger = logger.clone();
                                f.then(|_| {
                                    repeat(()).for_each(move |()| {
                                        let fut = tail.run();
                                        process_hook_results(fut, logger.clone()).and_then(|_| {
                                            sleep(Duration::new(10, 0)).map_err(|err| {
                                                format_err!("Tokio timer error {:?}", err)
                                            })
                                        })
                                    })
                                })
                                .boxify()
                            }
                            (_, Some(changeset)) => {
                                let fut = tail.run_single_changeset(changeset);
                                process_hook_results(fut, logger)
                            }
                            _ => {
                                let logger = logger.clone();
                                f.then(move |_| {
                                    let fut = tail.run_with_limit(limit);
                                    process_hook_results(fut, logger)
                                })
                                .boxify()
                            }
                        }
                    }
                })
        }
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(fut)
}

fn process_hook_results(
    fut: BoxFuture<Vec<HookResults>, Error>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    fut.and_then(move |res| {
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
                let output = format!(
                    "changeset:{} hook_name:{} path:{} result: {}",
                    exec_id.cs_id, exec_id.hook_name, exec_id.file.path, exec
                );

                match exec {
                    HookExecution::Accepted => debug!(logger, "{}", output),
                    HookExecution::Rejected(_) => info!(logger, "{}", output),
                }
            });
            debug!(logger, "==== Changeset hooks results ====");
            cs_hooks_result.into_iter().for_each(|(exec_id, exec)| {
                cs_hooks_stat.record_hook_execution(&exec);
                let output = format!(
                    "changeset:{} hook_name:{} result: {}",
                    exec_id.cs_id, exec_id.hook_name, exec
                );

                match exec {
                    HookExecution::Accepted => debug!(logger, "{}", output),
                    HookExecution::Rejected(_) => info!(logger, "{}", output),
                }
            });
        });

        info!(logger, "==== File hooks stat: {} ====", file_hooks_stat);
        info!(logger, "==== Changeset hooks stat: {} ====", cs_hooks_stat);

        if cs_hooks_stat.rejected > 0 || file_hooks_stat.rejected > 0 {
            err(err_msg(format!(
                "Hook rejections: changeset: {} file: {}",
                cs_hooks_stat.rejected, file_hooks_stat.rejected
            )))
        } else {
            ok(())
        }
    })
    .boxify()
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
    let app = App::new("mononoke hook server")
        .version("0.0.0")
        .about("run hooks against repo")
        .arg(
            Arg::with_name("cpath")
                .long("config_path")
                .short("P")
                .help("path to the config files")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("bookmark")
                .long("bookmark")
                .short("B")
                .help("bookmark to tail")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("repo_name")
                .long("repo_name")
                .short("R")
                .help("the name of the repo to run hooks for")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("changeset")
                .long("changeset")
                .short("c")
                .help("the changeset to run hooks for")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("exclude")
                .long("exclude")
                .short("e")
                .help("a comma separated list of changesets to exclude")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("exclude_file")
                .long("exclude_file")
                .short("f")
                .help("a file containing changesets to exclude that is separated by new lines")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("limit")
                .long("limit")
                .takes_value(true)
                .help("limit number of commits to process (non-continuous only). Default: 1000"),
        )
        .arg(
            Arg::with_name("continuous")
                .long("continuous")
                .help("continuously run hooks on new commits"),
        )
        .arg(
            Arg::with_name("init_revision")
                .long("init_revision")
                .takes_value(true)
                .help("the initial revision to start at"),
        )
        .arg(
            Arg::with_name("debug")
                .long("debug")
                .short("d")
                .help("print debug level output"),
        );

    let app = cmdlib::args::add_myrouter_args(app);
    cmdlib::args::add_cachelib_args(app, false /* hide_advanced_args */)
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
            GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer)
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
    #[fail(display = "No such repo '{}'", _0)]
    NoSuchRepo(String),
}
