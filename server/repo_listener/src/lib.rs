/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
// TODO(T33448938) use of deprecated item 'tokio::timer::Deadline': use Timeout instead
#![allow(deprecated)]
#![feature(never_type)]

use if_ as acl;

mod connection_acceptor;
mod errors;
mod repo_handlers;
mod request_handler;

use anyhow::Error;
use blobrepo_factory::{Caching, ReadOnlyStorage};
use configerator::ConfigSource;
use fbinit::FacebookInit;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;

use metaconfig_types::{CommonConfig, RepoConfig};

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

pub fn create_repo_listeners(
    fb: FacebookInit,
    common_config: CommonConfig,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    myrouter_port: Option<u16>,
    caching: Caching,
    disabled_hooks: &HashMap<String, HashSet<String>>,
    root_log: &Logger,
    sockname: &str,
    tls_acceptor: SslAcceptor,
    terminate_process: &'static AtomicBool,
    config_source: Option<ConfigSource>,
    readonly_storage: ReadOnlyStorage,
) -> (BoxFuture<(), Error>, ready_state::ReadyState) {
    let sockname = String::from(sockname);
    let root_log = root_log.clone();
    let mut ready = ready_state::ReadyStateBuilder::new();

    (
        repo_handlers(
            fb,
            repos,
            myrouter_port,
            caching,
            disabled_hooks,
            common_config.scuba_censored_table.clone(),
            readonly_storage,
            &root_log,
            &mut ready,
        )
        .and_then(move |handlers| {
            connection_acceptor(
                fb,
                common_config,
                sockname,
                root_log,
                handlers,
                tls_acceptor,
                terminate_process,
                config_source,
            )
        })
        .boxify(),
        ready.freeze(),
    )
}
