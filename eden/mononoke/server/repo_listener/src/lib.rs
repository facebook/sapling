/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
// TODO(T33448938) use of deprecated item 'tokio_old::timer::Deadline': use Timeout instead
#![allow(deprecated)]
#![feature(never_type)]

use infrasec_authorization as acl;

mod connection_acceptor;
mod errors;
mod repo_handlers;
mod request_handler;

pub use crate::connection_acceptor::wait_for_connections_closed;

use anyhow::Error;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use configerator_cached::ConfigStore;
use fbinit::FacebookInit;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::Future;
use openssl::ssl::SslAcceptor;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use std::collections::{HashMap, HashSet};
use std::sync::{atomic::AtomicBool, Arc};

use cmdlib::monitoring::ReadyFlagService;
use metaconfig_types::{CommonConfig, RepoConfig};

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

pub fn create_repo_listeners(
    fb: FacebookInit,
    common_config: CommonConfig,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    mysql_options: MysqlOptions,
    caching: Caching,
    disabled_hooks: HashMap<String, HashSet<String>>,
    root_log: &Logger,
    sockname: &str,
    tls_acceptor: SslAcceptor,
    service: ReadyFlagService,
    terminate_process: Arc<AtomicBool>,
    config_store: Option<ConfigStore>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
) -> BoxFuture<(), Error> {
    let sockname = String::from(sockname);
    let root_log = root_log.clone();

    repo_handlers(
        fb,
        repos,
        mysql_options,
        caching,
        disabled_hooks,
        common_config.scuba_censored_table.clone(),
        readonly_storage,
        blobstore_options.clone(),
        &root_log,
    )
    .and_then(move |handlers| {
        connection_acceptor(
            fb,
            common_config,
            sockname,
            service,
            root_log,
            handlers,
            tls_acceptor,
            terminate_process,
            config_store,
        )
    })
    .boxify()
}
