// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]
#![feature(try_from)]

extern crate dns_lookup;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate futures_stats;
#[macro_use]
extern crate maplit;
extern crate openssl;
#[macro_use]
extern crate slog;
extern crate slog_kvfilter;
extern crate slog_term;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;
extern crate tokio_openssl;
extern crate tracing;
extern crate uuid;

extern crate cache_warmup;
extern crate hgproto;
extern crate mercurial_types;
extern crate metaconfig;
extern crate ready_state;
extern crate repo_client;
extern crate scuba_ext;
extern crate sshrelay;

mod connection_acceptor;
mod errors;
mod repo_listen;

use std::collections::HashMap;

use futures::{future, Future, sync::mpsc};
use futures_ext::{BoxFuture, FutureExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;

use metaconfig::repoconfig::RepoConfig;

use connection_acceptor::connection_acceptor;
use errors::*;
use repo_listen::repo_listen;

pub fn start_repo_listeners<I>(
    repos: I,
    root_log: &Logger,
    sockname: &str,
    tls_acceptor: SslAcceptor,
) -> (BoxFuture<(), Error>, ready_state::ReadyState)
where
    I: IntoIterator<Item = (String, RepoConfig)>,
{
    // Given the list of paths to repos:
    // - create a thread for it
    // - initialize the repo
    // - wait for connections in that thread

    let sockname = String::from(sockname);
    let mut repo_senders = HashMap::new();
    let mut ready = ready_state::ReadyStateBuilder::new();

    let handles: Vec<_> = repos
        .into_iter()
        .filter(|(reponame, config)| {
            if !config.enabled {
                info!(root_log, "Repo {} not enabled", reponame)
            };
            config.enabled
        })
        .map(|(reponame, config)| {
            info!(root_log, "Start listening for repo {:?}", config.repotype);
            let ready_handle = ready.create_handle(reponame.as_ref());

            // Buffer size doesn't make much sense. `.send()` consumes the sender, so we clone
            // the sender. However each clone creates one more entry in the channel.
            let (sender, receiver) = mpsc::channel(1);
            repo_senders.insert(reponame.clone(), sender);
            // create a future for each repo and start listening for connections
            repo_listen(reponame, config, root_log.clone(), ready_handle, receiver)
        })
        .collect();

    let conn_acceptor_handle =
        connection_acceptor(&sockname, root_log.clone(), repo_senders, tls_acceptor);

    (
        future::join_all(handles)
            .join(conn_acceptor_handle)
            .map(|_: (Vec<()>, ())| ())
            .boxify(),
        ready.freeze(),
    )
}
