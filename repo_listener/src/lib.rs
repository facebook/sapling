// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]
#![feature(try_from)]

#[macro_use]
extern crate cloned;
extern crate dns_lookup;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate futures_stats;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate maplit;
extern crate openssl;
#[macro_use]
extern crate slog;
extern crate slog_kvfilter;
extern crate slog_term;
#[macro_use]
extern crate stats;
extern crate time_ext;
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
mod request_handler;
mod repo_handlers;

use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;

use metaconfig::repoconfig::RepoConfig;

use connection_acceptor::connection_acceptor;
use errors::*;
use repo_handlers::repo_handlers;

pub fn create_repo_listeners<I>(
    repos: I,
    root_log: &Logger,
    sockname: &str,
    tls_acceptor: SslAcceptor,
) -> (BoxFuture<(), Error>, ready_state::ReadyState)
where
    I: IntoIterator<Item = (String, RepoConfig)>,
{
    let sockname = String::from(sockname);
    let root_log = root_log.clone();
    let mut ready = ready_state::ReadyStateBuilder::new();

    (
        repo_handlers(repos, &root_log, &mut ready)
            .and_then(move |handles| connection_acceptor(sockname, root_log, handles, tls_acceptor))
            .boxify(),
        ready.freeze(),
    )
}
