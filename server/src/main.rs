// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]
#![feature(try_from)]

extern crate bookmarks;
#[macro_use]
extern crate failure_ext as failure;
extern crate fbwhoami;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate futures_stats;
#[macro_use]
extern crate futures_trace;
extern crate itertools;
extern crate tokio;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_uds;

extern crate rand;
extern crate uuid;

extern crate clap;

#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_kvfilter;
extern crate slog_logview;
extern crate slog_scuba;
extern crate slog_stats;
extern crate slog_term;

extern crate lz4;
#[macro_use]
extern crate maplit;

extern crate async_compression;
extern crate blobrepo;
extern crate bundle2_resolver;
extern crate bytes;
extern crate cache_warmup;
extern crate filenodes;
extern crate hgproto;
#[cfg(test)]
extern crate many_files_dirs;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate metaconfig;
extern crate pylz4;
extern crate repoinfo;
extern crate revset;
extern crate scuba;
extern crate services;
extern crate sshrelay;
extern crate stats;
extern crate time_ext;

mod errors;
mod listener;
mod monitoring;
mod repo;

use std::collections::HashMap;
use std::io;
use std::mem;
use std::ops::DerefMut;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use failure::SlogKVError;
use futures::{Future, IntoFuture, Sink, Stream};
use futures::sink::Wait;
use futures::sync::mpsc;
use futures_ext::FutureExt;
use futures_ext::asynchronize;

use clap::{App, ArgMatches};

use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_kvfilter::KVFilter;
use slog_logview::LogViewDrain;

use scuba::{ScubaClient, ScubaSample};

use blobrepo::BlobRepo;
use bytes::Bytes;
use hgproto::{sshproto, HgProtoHandler};
use mercurial_types::RepositoryId;
use metaconfig::RepoConfigs;
use metaconfig::repoconfig::RepoConfig;

use errors::*;

use listener::{ssh_server_mux, Stdio};
use monitoring::{ReadyHandle, ReadyState, ReadyStateBuilder};

struct SenderBytesWrite {
    chan: Wait<mpsc::Sender<Bytes>>,
}

impl io::Write for SenderBytesWrite {
    fn flush(&mut self) -> io::Result<()> {
        self.chan
            .flush()
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.chan
            .send(Bytes::from(buf))
            .map(|_| buf.len())
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }
}

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

fn start_repo_listeners<I>(
    repos: I,
    root_log: &Logger,
    sockname: &str,
) -> Result<(Vec<JoinHandle<!>>, ReadyState)>
where
    I: IntoIterator<Item = (String, RepoConfig)>,
{
    // Given the list of paths to repos:
    // - create a thread for it
    // - initialize the repo
    // - wait for connections in that thread

    let sockname = String::from(sockname);
    let mut repo_senders = HashMap::new();
    let mut ready = ReadyStateBuilder::new();

    let mut handles: Vec<_> = repos
        .into_iter()
        .map(|(reponame, config)| {
            info!(root_log, "Start listening for repo {:?}", config.repotype);
            let ready_handle = ready.create_handle(reponame.as_ref());

            // Buffer size doesn't make much sense. `.send()` consumes the sender, so we clone
            // the sender. However each clone creates one more entry in the channel.
            let (sender, receiver) = mpsc::channel(1);
            repo_senders.insert(reponame.clone(), sender);
            // start a thread for each repo to own the reactor and start listening for
            // connections and detach it
            thread::Builder::new()
                .name(format!("listener_{:?}", config.repotype))
                .spawn({
                    let root_log = root_log.clone();
                    move || repo_listen(reponame, config, root_log, ready_handle, receiver)
                })
                .map_err(Error::from)
        })
        .collect();

    let conn_acceptor_handle = thread::Builder::new()
        .name(format!("connection_acceptor"))
        .spawn({
            let root_log = root_log.clone();
            move || connection_acceptor(&sockname, root_log, repo_senders)
        })
        .map_err(Error::from);

    handles.push(conn_acceptor_handle);
    if handles.iter().any(Result::is_err) {
        for err in handles.into_iter().filter_map(Result::err) {
            crit!(root_log, "Failed to spawn listener thread"; SlogKVError(err));
        }
        bail_err!(ErrorKind::Initialization(
            "at least one of the listener threads failed to be spawned",
        ));
    }

    Ok((
        handles.into_iter().filter_map(Result::ok).collect(),
        ready.freeze(),
    ))
}

// This function accepts connections, reads Preamble and routes request to a thread responsible for
// a particular repo
fn connection_acceptor(
    sockname: &str,
    root_log: Logger,
    repo_senders: HashMap<String, mpsc::Sender<Stdio>>,
) -> ! {
    let mut core = tokio_core::reactor::Core::new().expect("failed to create tokio core");
    let remote = core.remote();
    let connection_acceptor = listener::listener(sockname)
        .expect("failed to create listener")
        .map_err(Error::from)
        .and_then({
            let root_log = root_log.clone();
            move |sock| {
                match sock.peer_addr() {
                    Ok(addr) => info!(root_log, "New connection from {:?}", addr),
                    Err(err) => {
                        error!(root_log, "Failed to get peer addr"; SlogKVError(Error::from(err)))
                    }
                };
                ssh_server_mux(sock, remote.clone()).map(Some).or_else({
                    let root_log = root_log.clone();
                    move |err| {
                        error!(root_log, "Error while reading preamble: {}", err);
                        Ok(None)
                    }
                })
            }
        })
        .for_each(move |maybe_stdio| {
            if maybe_stdio.is_none() {
                return Ok(()).into_future().boxify();
            }
            let stdio = maybe_stdio.unwrap();
            match repo_senders.get(&stdio.preamble.reponame) {
                Some(sender) => sender
                    .clone()
                    .send(stdio)
                    .map(|_| ())
                    .or_else({
                        let root_log = root_log.clone();
                        move |err| {
                            error!(
                                root_log,
                                "Failed to send request to a repo processing thread: {}", err
                            );
                            Ok(())
                        }
                    })
                    .boxify(),
                None => {
                    error!(root_log, "Unknown repo: {}", stdio.preamble.reponame);
                    Ok(()).into_future().boxify()
                }
            }
        });

    core.run(connection_acceptor)
        .expect("failure while running listener on tokio core");

    // The server is an infinite stream of connections
    unreachable!();
}

// Listener thread for a specific repo
fn repo_listen(
    reponame: String,
    config: RepoConfig,
    root_log: Logger,
    ready_handle: ReadyHandle,
    input_stream: mpsc::Receiver<Stdio>,
) -> ! {
    let mut core = tokio_core::reactor::Core::new().expect("failed to create tokio core");
    let scuba_table = config.scuba_table.clone();
    let (_, repo) = repo::init_repo(
        &root_log,
        &config.repotype,
        config.generation_cache_size,
        RepositoryId::new(config.repoid),
        config.scuba_table.clone(),
    ).expect("failed to initialize repo");

    let listen_log = root_log.new(o!("repo" => repo.path().clone()));

    let handle = core.handle();
    let repo = Arc::new(repo);

    let initial_warmup =
        cache_warmup::cache_warmup(repo.blobrepo(), config.cache_warmup, listen_log.clone())
            .map_err({
                let listen_log = listen_log.clone();
                move |err| {
                    error!(listen_log, "failed to warmup cache: {}", err);
                    ()
                }
            });
    let initial_warmup = ready_handle.wait_for(initial_warmup);

    let server = input_stream.for_each(move |stdio| {
        // Have a connection. Extract std{in,out,err} streams for socket
        let Stdio {
            stdin,
            stdout,
            stderr,
            preamble,
        } = stdio;

        let session_uuid = uuid::Uuid::new_v4();
        let connect_time = Instant::now();
        let wireproto_calls = Arc::new(Mutex::new(Vec::new()));

        let stderr_write = SenderBytesWrite {
            chan: stderr.clone().wait(),
        };
        let drain = slog_term::PlainSyncDecorator::new(stderr_write);
        let drain = slog_term::FullFormat::new(drain).build();
        let drain = KVFilter::new(drain, Level::Critical).only_pass_any_on_all_keys(hashmap! {
            "remote".into() => hashset!["true".into()],
        });
        let drain = slog::Duplicate::new(drain, listen_log.clone()).ignore_res();
        let conn_log = Logger::root(drain, o![]);

        let client_log = conn_log.new(o!("session_uuid" => format!("{}", session_uuid)));
        // Construct a hg protocol handler
        let proto_handler = HgProtoHandler::new(
            stdin,
            repo::RepoClient::new(repo.clone(), client_log),
            sshproto::HgSshCommandDecode,
            sshproto::HgSshCommandEncode,
            &conn_log,
            wireproto_calls.clone(),
        );

        // send responses back
        let endres = if preamble.reponame == reponame {
            proto_handler
                .map_err(Error::from)
                .forward(stdout)
                .map(|_| ())
                .boxify()
        } else {
            Err(ErrorKind::IncorrectRepoName(preamble.reponame).into())
                .into_future()
                .boxify()
        };

        // If we got an error at this point, then catch it, print a message and return
        // Ok (if we allow the Error to propagate further it will shutdown the listener
        // rather than just the connection). Unfortunately there's no way to print what the
        // actual failing command was.
        // TODO: (stash) T30523706 seems to leave the client hanging?
        let endres = endres.or_else({
            let conn_log = conn_log.clone();
            move |err| {
                error!(conn_log, "Command failed"; SlogKVError(err), "remote" => "true");
                Ok(())
            }
        });

        let request_future = match scuba_table {
            None => asynchronize(move || endres).map_err(|_: Error| ()).boxify(),
            Some(ref scuba_table) => {
                let scuba_table = scuba_table.clone();
                let repo_path = repo.path().clone();
                let wireproto_calls = wireproto_calls.clone();
                asynchronize(move || {
                    endres.map(move |_| {
                        let scuba_client = ScubaClient::new(scuba_table);

                        let mut wireproto_calls = wireproto_calls.lock().expect("lock poisoned");
                        let wireproto_calls = mem::replace(wireproto_calls.deref_mut(), Vec::new());

                        let mut sample = ScubaSample::new();
                        sample.add("session_uuid", format!("{}", session_uuid));
                        let elapsed_time = connect_time.elapsed();
                        let elapsed_ms = elapsed_time.as_secs() * 1000
                            + elapsed_time.subsec_nanos() as u64 / 1000000;
                        sample.add("time_elapsed_ms", elapsed_ms);
                        sample.add("wireproto_commands", wireproto_calls);
                        sample.add("repo", repo_path);

                        scuba_client.log(&sample);
                    })
                }).map_err(|_| ())
                    .boxify()
            }
        };
        // Run the whole future asynchronously to allow new connections
        // Don't wait for more that 15 mins for a request
        handle.spawn(
            request_future
                .select(
                    tokio::timer::Delay::new(Instant::now() + Duration::from_secs(900))
                        .map({
                            let conn_log = conn_log.clone();
                            move |_| {
                                let mut wireproto_calls =
                                    wireproto_calls.lock().expect("lock poisoned");
                                let wireproto_calls =
                                    mem::replace(wireproto_calls.deref_mut(), Vec::new());
                                error!(conn_log, "timeout while handling {:?}", wireproto_calls);
                                ()
                            }
                        })
                        .map_err(|_| ()),
                )
                .map(|_| ())
                .map_err(|_| ()),
        );

        Ok(())
    });

    let server = server.join(initial_warmup);
    core.run(server)
        .expect("failure while running listener on tokio core");

    // The server is an infinite stream of connections
    unreachable!();
}

fn main() {
    setup_panic_hook();
    let matches = setup_app().get_matches();
    let root_log = setup_logger(&matches);

    fn run_server<'a>(root_log: &Logger, matches: ArgMatches<'a>) -> Result<!> {
        info!(root_log, "Starting up");

        let stats_aggregation = monitoring::start_stats()?;

        let config = get_config(root_log, &matches)?;
        let (repo_listeners, ready) = start_repo_listeners(
            config.repos.into_iter(),
            root_log,
            matches
                .value_of("listening-host-port")
                .expect("listening path must be specified"),
        )?;

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
