// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
// TODO(T33448938) use of deprecated item 'tokio::timer::Deadline': use Timeout instead
#![allow(deprecated)]
#![feature(never_type)]

extern crate aclchecker;

extern crate blobrepo_factory;
extern crate blobstore;
extern crate bytes;
extern crate if_ as acl;
#[macro_use]
extern crate cloned;
extern crate context;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate futures_stats;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate maplit;
extern crate openssl;
#[macro_use]
extern crate slog;
extern crate slog_ext;
extern crate slog_kvfilter;
extern crate slog_term;
extern crate sql;
#[macro_use]
extern crate stats;
extern crate time_ext;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;
extern crate tokio_openssl;
extern crate tokio_timer;
#[macro_use]
extern crate tracing;
extern crate uuid;

extern crate cache_warmup;
extern crate hgproto;
extern crate hooks;
extern crate hooks_content_stores;
extern crate metaconfig_types;
extern crate mononoke_types;
extern crate phases;
extern crate reachabilityindex;
extern crate ready_state;
extern crate repo_client;

extern crate scuba_ext;
extern crate skiplist;
extern crate sshrelay;
extern crate x509;

mod connection_acceptor;
mod errors;
mod repo_handlers;
mod request_handler;

use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;
use std::sync::atomic::AtomicBool;

use metaconfig_types::{CommonConfig, RepoConfig};

use crate::connection_acceptor::connection_acceptor;
use crate::errors::*;
use crate::repo_handlers::repo_handlers;

pub fn create_repo_listeners(
    common_config: CommonConfig,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
    root_log: &Logger,
    sockname: &str,
    tls_acceptor: SslAcceptor,
    terminate_process: &'static AtomicBool,
) -> (BoxFuture<(), Error>, ready_state::ReadyState) {
    let sockname = String::from(sockname);
    let root_log = root_log.clone();
    let mut ready = ready_state::ReadyStateBuilder::new();

    (
        repo_handlers(repos, myrouter_port, &root_log, &mut ready)
            .and_then(move |handlers| {
                connection_acceptor(
                    common_config,
                    sockname,
                    root_log,
                    handlers,
                    tls_acceptor,
                    terminate_process,
                )
            })
            .boxify(),
        ready.freeze(),
    )
}
