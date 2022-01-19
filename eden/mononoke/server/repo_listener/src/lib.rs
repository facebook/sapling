/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
mod wireproto_sink;

pub use crate::connection_acceptor::wait_for_connections_closed;

use anyhow::{Context as _, Result};
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use mononoke_api::Mononoke;
use openssl::ssl::SslAcceptor;
use rate_limiting::RateLimitEnvironment;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{o, Logger};
use sql_ext::facebook::MysqlOptions;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use blobstore_factory::BlobstoreOptions;
use cmdlib::monitoring::ReadyFlagService;
use metaconfig_types::CommonConfig;

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;

const CONFIGERATOR_RATE_LIMITING_CONFIG: &str = "scm/mononoke/ratelimiting/ratelimits";

pub async fn create_repo_listeners<'a>(
    fb: FacebookInit,
    common_config: CommonConfig,
    mononoke: Mononoke,
    blobstore_options: &'a BlobstoreOptions,
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
    cslb_config: Option<String>,
    bound_addr_file: Option<PathBuf>,
) -> Result<()> {
    let rate_limiter = {
        let handle = config_store
            .get_config_handle_DEPRECATED(CONFIGERATOR_RATE_LIMITING_CONFIG.to_string())
            .ok();

        handle.and_then(|handle| {
            common_config
                .loadlimiter_category
                .clone()
                .map(|category| RateLimitEnvironment::new(fb, category, handle))
        })
    };

    let handlers = repo_handlers(
        fb,
        &mononoke,
        blobstore_options,
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
            rate_limiter.clone(),
        )
        .context("Error instantiating EdenAPI")?
    };

    connection_acceptor(
        fb,
        common_config,
        sockname,
        service,
        root_log,
        handlers,
        tls_acceptor,
        terminate_process,
        rate_limiter,
        scribe,
        edenapi,
        will_exit,
        config_store,
        cslb_config,
        {
            let mut scuba = scuba.clone();
            scuba.add("service", "wireproto");
            scuba
        },
        bound_addr_file,
    )
    .await
}
