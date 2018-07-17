// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::mem;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dns_lookup::getnameinfo;
use failure::SlogKVError;
use futures::{Future, IntoFuture, Sink, Stream};
use futures::sync::mpsc;
use futures_ext::{asynchronize, FutureExt};
use futures_stats::Timed;
use slog::{self, Drain, Level, Logger};
use slog_kvfilter::KVFilter;
use slog_term;
use tokio::util::FutureExt as TokioFutureExt;
use tokio_core::reactor::Core;
use tracing::TraceContext;
use uuid::Uuid;

use cache_warmup::cache_warmup;
use hgproto::{sshproto, HgProtoHandler};
use mercurial_types::RepositoryId;
use metaconfig::repoconfig::RepoConfig;
use ready_state::ReadyHandle;
use repo_client::{MononokeRepo, RepoClient};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use sshrelay::{SenderBytesWrite, Stdio};

use errors::*;

/// Listener thread for a specific repo
pub fn repo_listen(
    reponame: String,
    config: RepoConfig,
    root_log: Logger,
    ready_handle: ReadyHandle,
    input_stream: mpsc::Receiver<(Stdio, SocketAddr)>,
) -> ! {
    let mut core = Core::new().expect("failed to create tokio core");

    let scuba_logger = ScubaSampleBuilder::with_opt_table(config.scuba_table.clone());

    let repo = MononokeRepo::new(
        root_log.new(o!("repo" => reponame.clone())),
        &config.repotype,
        RepositoryId::new(config.repoid),
    ).expect(&format!("failed to initialize repo {}", reponame));

    let listen_log = root_log.new(o!("repo" => repo.path().clone()));

    let handle = core.handle();
    let repo = Arc::new(repo);

    let initial_warmup = cache_warmup(repo.blobrepo(), config.cache_warmup, listen_log.clone())
        .map_err({
            let listen_log = listen_log.clone();
            move |err| {
                error!(listen_log, "failed to warmup cache: {}", err);
                ()
            }
        });
    let initial_warmup = ready_handle.wait_for(initial_warmup);

    let server = input_stream.for_each(move |(stdio, addr)| {
        // Have a connection. Extract std{in,out,err} streams for socket
        let Stdio {
            stdin,
            stdout,
            stderr,
            mut preamble,
        } = stdio;

        let session_uuid = match preamble
            .misc
            .get("session_uuid")
            .and_then(|v| Uuid::parse_str(v).ok())
        {
            Some(session_uuid) => session_uuid,
            None => {
                let session_uuid = Uuid::new_v4();
                preamble
                    .misc
                    .insert("session_uuid".to_owned(), format!("{}", session_uuid));
                session_uuid
            }
        };

        let wireproto_calls = Arc::new(Mutex::new(Vec::new()));
        let trace = TraceContext::new(session_uuid, Instant::now());

        let conn_log = {
            let stderr_write = SenderBytesWrite {
                chan: stderr.clone().wait(),
            };
            let client_drain = slog_term::PlainSyncDecorator::new(stderr_write);
            let client_drain = slog_term::FullFormat::new(client_drain).build();
            let client_drain = KVFilter::new(client_drain, Level::Critical)
                .only_pass_any_on_all_keys(hashmap! {
                    "remote".into() => hashset!["true".into(), "remote_only".into()],
                });

            let server_drain = KVFilter::new(listen_log.clone(), Level::Critical)
                .always_suppress_any(hashmap! {
                    "remote".into() => hashset!["remote_only".into()],
                });

            let drain = slog::Duplicate::new(client_drain, server_drain).ignore_res();
            Logger::root(drain, o!("session_uuid" => format!("{}", session_uuid)))
        };

        let mut scuba_logger = {
            let client_hostname = match getnameinfo(&addr, 0) {
                Ok((hostname, _)) => hostname,
                Err(err) => {
                    warn!(
                        conn_log,
                        "failed to lookup hostname for address {}, reason: {:?}", addr, err
                    );
                    "".to_owned()
                }
            };
            let mut scuba_logger = scuba_logger.clone();
            scuba_logger
                .add_preamble(&preamble)
                .add("client_hostname", client_hostname);
            scuba_logger
        };

        scuba_logger.log_with_msg("Connection established", None);

        // Construct a hg protocol handler
        let proto_handler = HgProtoHandler::new(
            stdin,
            RepoClient::new(repo.clone(), conn_log.clone(), scuba_logger.clone(), trace),
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

        // Don't wait for more that 15 mins for a request
        let endres = endres
            .deadline(Instant::now() + Duration::from_secs(900))
            .timed(move |stats, result| {
                let mut wireproto_calls = wireproto_calls.lock().expect("lock poisoned");
                let wireproto_calls = mem::replace(wireproto_calls.deref_mut(), Vec::new());

                scuba_logger
                    .add_stats(&stats)
                    .add("wireproto_commands", wireproto_calls);

                match result {
                    Ok(_) => scuba_logger.log_with_msg("Request finished - Success", None),
                    Err(err) => if err.is_inner() {
                        scuba_logger
                            .log_with_msg("Request finished - Failure", format!("{:#?}", err));
                    } else if err.is_elapsed() {
                        scuba_logger.log_with_msg("Request finished - Timeout", None);
                    } else {
                        scuba_logger.log_with_msg(
                            "Request finished - Unexpected timer error",
                            format!("{:#?}", err),
                        );
                    },
                }
                Ok(())
            })
            .map_err(move |err| {
                if err.is_inner() {
                    error!(conn_log, "Command failed";
                        SlogKVError(err.into_inner().unwrap()),
                        "remote" => "true");
                } else if err.is_elapsed() {
                    error!(conn_log, "Timeout while handling request";
                        "remote" => "true");
                } else {
                    crit!(conn_log, "Unexpected error";
                        SlogKVError(err.into_timer().unwrap().into()),
                        "remote" => "true");
                }
                format_err!("This is a dummy error, not supposed to be catched")
            });

        // Make this double async.
        // TODO(stash, luk) is this really necessary?
        handle.spawn(asynchronize(move || endres).then(|_| Ok(())));

        Ok(())
    });

    let server = server.join(initial_warmup);
    core.run(server)
        .expect("failure while running listener on tokio core");

    // The server is an infinite stream of connections
    unreachable!();
}
