/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(never_type)]
#![recursion_limit = "256"]

mod connection_acceptor;
mod errors;
mod repo_handlers;
mod request_handler;
mod security_checker;

pub use crate::connection_acceptor::wait_for_connections_closed;

use anyhow::Result;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::compat::Future01CompatExt;
use openssl::ssl::SslAcceptor;
use scribe_ext::Scribe;
use slog::{debug, Logger};
use sql_ext::facebook::MysqlOptions;
use std::collections::{HashMap, HashSet};

use cmdlib::monitoring::ReadyFlagService;
use metaconfig_types::{CommonConfig, RepoConfig};

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

pub async fn create_repo_listeners(
    fb: FacebookInit,
    common_config: CommonConfig,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    mysql_options: MysqlOptions,
    caching: Caching,
    disabled_hooks: HashMap<String, HashSet<String>>,
    root_log: Logger,
    sockname: String,
    tls_acceptor: SslAcceptor,
    service: ReadyFlagService,
    terminate_process: oneshot::Receiver<()>,
    config_store: Option<ConfigStore>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    scribe: Scribe,
) -> Result<()> {
    let handlers = repo_handlers(
        fb,
        repos,
        mysql_options,
        caching,
        disabled_hooks,
        common_config.scuba_censored_table.clone(),
        readonly_storage,
        blobstore_options,
        &root_log,
    )
    .compat()
    .await?;

    debug!(root_log, "Mononoke server is listening on {}", sockname);
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
        scribe,
    )
    .await
}
