/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]
#![feature(never_type)]
#![recursion_limit = "256"]

mod connection_acceptor;
mod errors;
mod http_service;
mod netspeedtest;
mod repo_handlers;
mod request_handler;
mod wireproto_sink;

pub use crate::connection_acceptor::wait_for_connections_closed;

use crate::connection_acceptor::connection_acceptor;
use crate::repo_handlers::repo_handlers;
use anyhow::Context as _;
use anyhow::Result;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use cmdlib::monitoring::ReadyFlagService;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use metaconfig_types::CommonConfig;
use mononoke_api::Mononoke;
use openssl::ssl::SslAcceptor;
use permission_checker::AclProvider;
use rate_limiting::RateLimitEnvironment;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::o;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const CONFIGERATOR_RATE_LIMITING_CONFIG: &str = "scm/mononoke/ratelimiting/ratelimits";

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
    cslb_config: Option<String>,
    bound_addr_file: Option<PathBuf>,
    acl_provider: &dyn AclProvider,
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
        mysql_options,
        readonly_storage,
        &root_log,
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
            &common_config,
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
        acl_provider,
    )
    .await
}
