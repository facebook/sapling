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
mod http_service;
mod netspeedtest;
mod repo_handlers;
mod request_handler;
mod security_checker;
mod stream;

pub use crate::connection_acceptor::wait_for_connections_closed;

use anyhow::Result;
use blobrepo_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use mononoke_api::Mononoke;
use openssl::ssl::SslAcceptor;
use scribe_ext::Scribe;
use slog::{debug, Logger};
use sql_ext::facebook::MysqlOptions;

use cmdlib::monitoring::ReadyFlagService;
use metaconfig_types::CommonConfig;
use observability::ObservabilityContext;

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

pub async fn create_repo_listeners<'a>(
    fb: FacebookInit,
    test_instance: bool,
    common_config: CommonConfig,
    repos: Mononoke,
    mysql_options: &'a MysqlOptions,
    root_log: Logger,
    sockname: String,
    tls_acceptor: SslAcceptor,
    service: ReadyFlagService,
    terminate_process: oneshot::Receiver<()>,
    config_store: &'a ConfigStore,
    readonly_storage: ReadOnlyStorage,
    scribe: Scribe,
    observability_context: &'static ObservabilityContext,
) -> Result<()> {
    let handlers = repo_handlers(
        fb,
        repos,
        mysql_options,
        readonly_storage,
        &root_log,
        config_store,
        observability_context,
    )
    .await?;

    debug!(root_log, "Mononoke server is listening on {}", sockname);
    connection_acceptor(
        fb,
        test_instance,
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
