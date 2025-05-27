/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(associated_type_defaults)]
#![feature(let_chains)]
#![feature(try_blocks)]

pub mod context;
mod errors;
pub mod handlers;
pub mod middleware;
mod scuba;
pub mod utils;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Error;
use clientinfo::ClientEntryPoint;
use fbinit::FacebookInit;
use gotham::router::Router;
use gotham_ext::handler::MononokeHttpHandler;
use gotham_ext::middleware::ConfigInfoMiddleware;
use gotham_ext::middleware::LoadMiddleware;
use gotham_ext::middleware::LogMiddleware;
use gotham_ext::middleware::MetadataMiddleware;
use gotham_ext::middleware::PostResponseMiddleware;
use gotham_ext::middleware::RequestContextMiddleware;
use gotham_ext::middleware::ScubaMiddleware;
use gotham_ext::middleware::ServerIdentityMiddleware;
use gotham_ext::middleware::TimerMiddleware;
use gotham_ext::middleware::TlsSessionDataMiddleware;
use http::HeaderValue;
use metaconfig_types::CommonConfig;
use mononoke_api::Mononoke;
use mononoke_configs::MononokeConfigs;
use rate_limiting::RateLimitEnvironment;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

use crate::context::ServerContext;
use crate::handlers::build_router;
use crate::middleware::OdsMiddleware;
use crate::middleware::RequestDumperMiddleware;
use crate::middleware::ThrottleMiddleware;
use crate::scuba::SaplingRemoteApiScubaHandler;

pub type SaplingRemoteApi = MononokeHttpHandler<Router>;

pub fn build<R: Send + Sync + Clone + 'static>(
    fb: FacebookInit,
    logger: Logger,
    scuba: MononokeScubaSampleBuilder,
    mononoke: Arc<Mononoke<R>>,
    will_exit: Arc<AtomicBool>,
    test_friendly_loging: bool,
    tls_session_data_log_path: Option<&Path>,
    rate_limiter: Option<RateLimitEnvironment>,
    configs: Arc<MononokeConfigs>,
    common_config: &CommonConfig,
    readonly: bool,
    mtls_disabled: bool,
) -> Result<SaplingRemoteApi, Error> {
    let ctx = ServerContext::new(mononoke, will_exit);

    let log_middleware = if test_friendly_loging {
        LogMiddleware::test_friendly()
    } else {
        LogMiddleware::slog(logger.clone())
    };

    // Set up the router and handler for serving HTTP requests, along with custom middleware.
    // The middleware added here does not implement Gotham's usual Middleware trait; instead,
    // it uses the custom Middleware API defined in the gotham_ext crate. Native Gotham
    // middleware is set up during router setup in build_router.
    let router = build_router(ctx);

    let handler = MononokeHttpHandler::builder()
        .add(TlsSessionDataMiddleware::new(tls_session_data_log_path)?)
        .add(ConfigInfoMiddleware::new(configs))
        .add(MetadataMiddleware::new(
            fb,
            logger.clone(),
            common_config.internal_identity.clone(),
            ClientEntryPoint::SaplingRemoteApi,
            mtls_disabled,
        ))
        .add(ServerIdentityMiddleware::new(HeaderValue::from_static(
            "edenapi_server",
        )))
        .add(PostResponseMiddleware::default())
        .add(RequestContextMiddleware::new(
            fb,
            logger,
            scuba.clone(),
            rate_limiter,
            readonly,
        ))
        .add(RequestDumperMiddleware::new(
            fb,
            common_config.edenapi_dumper_scuba_table.clone(),
        ))
        .add(ThrottleMiddleware::new())
        .add(LoadMiddleware::new())
        .add(log_middleware)
        .add(OdsMiddleware::new())
        .add(<ScubaMiddleware<SaplingRemoteApiScubaHandler>>::new(scuba))
        .add(TimerMiddleware::new())
        .build(router);

    Ok(handler)
}
