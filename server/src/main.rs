// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate futures;
extern crate bytes;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_uds;

extern crate clap;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate kvfilter;

#[macro_use]
extern crate maplit;

extern crate async_compression;
extern crate bookmarks;
extern crate hgproto;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
extern crate sshrelay;
extern crate futures_ext;

use std::thread;
use std::path::{Path, PathBuf};
use std::panic;
use std::sync::Arc;
use std::io;

use futures::{Future, Sink, Stream};
use futures::sink::Wait;
use futures::sync::mpsc;

use clap::App;

use slog::{Drain, Level, LevelFilter, Logger};
use kvfilter::KVFilter;

use bytes::Bytes;
use hgproto::sshproto::{HgSshCommandDecode, HgSshCommandEncode};
use hgproto::HgService;
use errors::ResultExt;
use futures_ext::{StreamLayeredExt, encode};

mod errors;
mod repo;
mod listener;

use errors::*;

use listener::{Stdio, ssh_server_mux};

fn init_repo<P: AsRef<Path>>(parent_logger: &Logger, repopath: P) -> Result<(PathBuf, repo::Repo)> {
    let repopath = repopath.as_ref();

    let mut sock = PathBuf::from(repopath);
    sock.push(".hg");

    let repo = repo::Repo::new(parent_logger, &sock)
        .chain_err(|| format!("Failed to initialize repo {:?}", repopath))?;

    sock.push("mononoke.sock");

    Ok((sock, repo))
}

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

// Listener thread for a specific repo
fn repo_listen(sockname: &Path, repo: repo::Repo, listen_log: &Logger) -> Result<()> {
    let mut core = tokio_core::reactor::Core::new()?;
    let handle = core.handle();
    let repo = Arc::new(repo);

    let server = listener::listener(sockname, &handle)?
        .map_err(Error::from)
        .for_each(move |sock| {
            match sock.peer_addr() {
                Ok(addr) => info!(listen_log, "New connection from {:?}", addr),
                Err(err) => error!(listen_log, "Failed to get peer addr: {}", err),
            };

            // Have a connection. Extract std{in,out,err} streams for socket
            let Stdio {
                stdin,
                stdout,
                stderr,
            } = ssh_server_mux(sock, &handle);

            let stderr_write = SenderBytesWrite { chan: stderr.clone().wait() };
            let drain = slog_term::PlainSyncDecorator::new(stderr_write);
            let drain = slog_term::FullFormat::new(drain).build();
            let drain = KVFilter::new(
                drain,
                Level::Critical,
                vec![(String::from("remote"), hashset![String::from("true")])],
            );
            let drain = slog::Duplicate::new(drain, listen_log.clone()).fuse();
            let conn_log = slog::Logger::root(drain, o![]);

            // Construct a repo
            let client = repo::RepoClient::new(repo.clone(), &conn_log);
            let service = Arc::new(HgService::new_with_logger(client, &conn_log));

            // Map stdin into mercurial requests
            let reqs = stdin.decode(HgSshCommandDecode);

            // process requests
            let resps = reqs.and_then(move |req| service.clone().command(req));

            // send responses back
            let endres = encode::encode(resps, HgSshCommandEncode)
                .map_err(Error::from)
                .forward(stdout)
                .map(|_| ());

            // If we got an error at this point, then catch it, print a message and return
            // Ok (if we allow the Error to propagate further it will shutdown the listener
            // rather than just the connection). Unfortunately there's no way to print what the
            // actual failing command was.
            // TODO: seems to leave the client hanging?
            let conn_log = conn_log.clone();
            let endres = endres.or_else(move |err| {
                error!(conn_log, "Command failed: {}", err; "remote" => "true");

                for e in err.iter().skip(1) {
                    error!(conn_log, "caused by: {}", e; "remote" => "true");
                }
                Ok(())
            });

            // Run the whole future asynchronously to allow new connections
            handle.spawn(endres);

            Ok(())
        });

    core.run(server)?;

    Ok(())
}

fn run<'a, I>(repos: I, root_log: &Logger) -> Result<()>
where
    I: IntoIterator<Item = &'a str>,
{
    // Given the list of paths to repos:
    // - initialize the repo
    // - create a thread for it
    // - wait for connections in that thread
    let threads = repos
        .into_iter()
        .map(|repopath| {
            init_repo(root_log, &repopath).and_then(|(sockname, repo)| {
                let repopath = PathBuf::from(repopath);
                let listen_log = root_log.new(o!("repo" => format!("{:?}", repopath)));
                info!(listen_log, "Listening for connections");

                // start a thread for each repo to own the reactor and start listening for
                // connections
                let t = thread::spawn(move || {
                    // Not really sure this is actually Unwind Safe
                    // (future version of slog will make this explicit)
                    let unw = panic::catch_unwind(panic::AssertUnwindSafe(
                        || repo_listen(&sockname, repo, &listen_log),
                    ));
                    match unw {
                        Err(err) => {
                            crit!(
                                listen_log,
                                "Listener thread {:?} paniced: {:?}",
                                repopath,
                                err
                            );
                            Ok(())
                        }
                        Ok(v) => v,
                    }
                });
                Ok(t)
            })
        })
        .collect::<Vec<_>>();

    // Check for an report any repo initialization errors
    for err in threads.iter().filter_map(|t| t.as_ref().err()) {
        error!(root_log, "{}", err);
        for chain_link in err.iter().skip(1) {
            error!(root_log, "Reason: {}", chain_link)
        }
    }

    // Wait for all threads, and report any problem they have
    for thread in threads.into_iter().filter_map(Result::ok) {
        if let Err(err) = thread.join().expect("thread join failed") {
            error!(root_log, "Listener failure: {:?}", err);
        }
    }

    Ok(())
}

fn main() {
    let matches = App::new("mononoke server")
        .version("0.0.0")
        .about("serve repos")
        .args_from_usage("[debug] -d, --debug     'print debug level output'")
        .args_from_usage(
            "<REPODIR>...            'paths to repo dirs (parent of .hg)'",
        )
        .get_matches();

    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    // TODO: switch to TermDecorator, which supports color
    let drain = slog_term::PlainSyncDecorator::new(io::stdout());
    let drain = slog_term::FullFormat::new(drain).build();
    let drain = LevelFilter::new(drain, level).fuse();
    let root_log = slog::Logger::root(drain, o![]);

    info!(root_log, "Starting up");

    let repos = matches.values_of("REPODIR").unwrap();

    if let Err(ref e) = run(repos, &root_log) {
        error!(root_log, "Failed: {}", e);

        for e in e.iter().skip(1) {
            error!(root_log, "caused by: {}", e);
        }

        std::process::exit(1);
    }
}
