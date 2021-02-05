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

use anyhow::{Context as _, Result};
use blobrepo_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use load_limiter::LoadLimiterEnvironment;
use mononoke_api::Mononoke;
use openssl::ssl::SslAcceptor;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{debug, o, Logger};
use sql_ext::facebook::MysqlOptions;
use std::sync::{atomic::AtomicBool, Arc};

use cmdlib::monitoring::ReadyFlagService;
use metaconfig_types::CommonConfig;

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

const CONFIGERATOR_LIMITS_CONFIG: &str = "scm/mononoke/loadshedding/limits";

pub async fn create_repo_listeners<'a>(
    fb: FacebookInit,
    common_config: CommonConfig,
    mononoke: Mononoke,
    mysql_options: &'a MysqlOptions,
    root_log: Logger,
    sockname: String,
    tls_acceptor: SslAcceptor,
    service: ReadyFlagService,
    terminate_process: oneshot::Receiver<()>,
    config_store: &'a ConfigStore,
    readonly_storage: ReadOnlyStorage,
    scribe: Scribe,
    scuba: &'a MononokeScubaSampleBuilder,
    will_exit: Arc<AtomicBool>,
) -> Result<()> {
    let load_limiter = {
        let handle = config_store
            .get_config_handle(CONFIGERATOR_LIMITS_CONFIG.to_string())
            .ok();

        handle.and_then(|handle| {
            common_config
                .loadlimiter_category
                .clone()
                .map(|category| LoadLimiterEnvironment::new(fb, category, handle))
        })
    };

    let handlers = repo_handlers(
        fb,
        &mononoke,
        mysql_options,
        readonly_storage,
        &root_log,
        config_store,
        scuba,
    )
    .await?;

    let edenapi = {
        let mut scuba = scuba.clone();
        scuba.add("service", "edenapi");

        edenapi_service::build(
            fb,
            root_log.new(o!("service" => "edenapi")),
            scuba,
            mononoke,
            will_exit.clone(),
            false,
            None,
            load_limiter.clone(),
        )
        .context("Error instantiating EdenAPI")?
    };

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
        load_limiter,
        scribe,
        edenapi,
        will_exit,
    )
    .await
}
