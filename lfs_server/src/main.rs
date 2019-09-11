// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(async_await)]
#![feature(async_closure)]
#![deny(warnings)]

use clap::Arg;
use failure::{err_msg, Error};
use futures::{Future, IntoFuture};
use futures_preview::{FutureExt, TryFutureExt};
use futures_util::{compat::Future01CompatExt, try_future::try_join_all};
use gotham::{
    bind_server,
    handler::{HandlerError, HandlerFuture, IntoHandlerError},
    helpers::http::response::create_response,
    middleware::{logger::RequestLogger, state::StateMiddleware},
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::{request_id, State},
};
use hyper::{header::HeaderValue, Body, Response, StatusCode};
use mime::Mime;
use slog::warn;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use tokio::net::TcpListener;
use tokio_openssl::SslAcceptorExt;

use blobrepo_factory::open_blobrepo;
use failure_ext::chain::ChainExt;
use metaconfig_parser::RepoConfigs;
use mononoke_types::RepositoryId;

use cmdlib::args;

use crate::http::{git_lfs_mime, HttpError};
use lfs_server_context::{LfsServerContext, ServerUris};
use protocol::ResponseError;

mod batch;
mod download;
mod errors;
mod lfs_server_context;
mod protocol;
mod str_serialized;
mod upload;
#[macro_use]
mod http;

// TODO: left to do here:
// - HTTPS
// - Logging
// - VIP-level routing (won't happen in this code base, though)
// - Verify that we are talking HTTP/2 to upstream
// - Make upstream optional for tests?

const ARG_SELF_URL: &str = "self-url";
const ARG_UPSTREAM_URL: &str = "upstream-url";
const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";
const ARG_TLS_CERTIFICATE: &str = "tls-certificate";
const ARG_TLS_PRIVATE_KEY: &str = "tls-private-key";
const ARG_TLS_CA: &str = "tls-ca";
const ARG_TLS_TICKET_SEEDS: &str = "tls-ticket-seeds";

async fn build_response(
    res: Result<(Body, Mime), HttpError>,
    state: State,
) -> Result<(State, Response<Body>), (State, HandlerError)> {
    let mut res = match res {
        Ok((body, mime)) => create_response(&state, StatusCode::OK, mime, body),
        Err(error) => {
            let HttpError { error, status_code } = error;

            let res = ResponseError {
                message: error.to_string(),
                documentation_url: None,
                request_id: Some(request_id(&state).to_string()),
            };

            // Bail if we can't convert the response to json.
            match serde_json::to_string(&res) {
                Ok(res) => create_response(&state, status_code, git_lfs_mime(), res),
                Err(error) => return Err((state, error.into_handler_error())),
            }
        }
    };

    let headers = res.headers_mut();
    headers.insert("Server", HeaderValue::from_static("mononoke-lfs"));
    Ok((state, res))
}

// These 3 methods are wrappers to go from async fn's to the implementations Gotham expects,
// as well as creating HTTP responses using build_response().
fn batch_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = batch::batch(&mut state).await;
            build_response(res, state).await
        })()
        .boxed()
        .compat(),
    )
}

fn download_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = download::download(&mut state).await;
            build_response(res, state).await
        })()
        .boxed()
        .compat(),
    )
}

fn upload_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = upload::upload(&mut state).await;
            build_response(res, state).await
        })()
        .boxed()
        .compat(),
    )
}

fn health_handler(state: State) -> (State, &'static str) {
    (state, "I_AM_ALIVE")
}

fn router(lfs_ctx: LfsServerContext) -> Router {
    let pipeline = new_pipeline()
        .add(StateMiddleware::new(lfs_ctx))
        .add(RequestLogger::new(::log::Level::Info))
        .build();
    let (chain, pipelines) = single_pipeline(pipeline);

    build_router(chain, pipelines, |route| {
        route
            .post("/:repository/objects/batch")
            .with_path_extractor::<batch::BatchParams>()
            .to(batch_handler);

        route
            .get("/:repository/download/:content_id")
            .with_path_extractor::<download::DownloadParams>()
            .to(download_handler);

        route
            .put("/:repository/upload/:oid/:size")
            .with_path_extractor::<upload::UploadParams>()
            .to(upload_handler);

        route.get("/health_check").to(health_handler);
    })
}

fn main() -> Result<(), Error> {
    let app = args::MononokeApp {
        hide_advanced_args: true,
        default_glog: true,
    }
    .build("Mononoke LFS Server")
    .arg(
        Arg::with_name(ARG_LISTEN_HOST)
            .long("--listen-host")
            .takes_value(true)
            .default_value("127.0.0.1")
            .help("The host to listen on locally"),
    )
    .arg(
        Arg::with_name(ARG_LISTEN_PORT)
            .long("--listen-port")
            .takes_value(true)
            .default_value("8001")
            .help("The port to listen on locally"),
    )
    .arg(
        Arg::with_name(ARG_TLS_CERTIFICATE)
            .long("--tls-certificate")
            .takes_value(true),
    )
    .arg(
        Arg::with_name(ARG_TLS_PRIVATE_KEY)
            .long("--tls-private-key")
            .takes_value(true),
    )
    .arg(
        Arg::with_name(ARG_TLS_CA)
            .long("--tls-ca")
            .takes_value(true),
    )
    .arg(
        Arg::with_name(ARG_TLS_TICKET_SEEDS)
            .long("--tls-ticket-seeds")
            .takes_value(true),
    )
    .arg(
        Arg::with_name(ARG_SELF_URL)
            .takes_value(true)
            .required(true)
            .help("The base URL for this server"),
    )
    .arg(
        Arg::with_name(ARG_UPSTREAM_URL)
            .takes_value(true)
            .help("The base URL for an upstream server"),
    );
    let app = args::add_fb303_args(app);

    let matches = app.get_matches();

    let caching = args::init_cachelib(&matches);
    let logger = args::get_logger(&matches);
    let myrouter_port = args::parse_myrouter_port(&matches);

    // NOTE: This captures stdlogs using slog.
    let log_guard = slog_scope::set_global_logger(logger.clone());
    slog_stdlog::init_with_level(::log::Level::Info)?;

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();

    let tls_certificate = matches.value_of(ARG_TLS_CERTIFICATE);
    let tls_private_key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let tls_ca = matches.value_of(ARG_TLS_CA);
    let tls_ticket_seeds = matches.value_of(ARG_TLS_TICKET_SEEDS);

    let server = ServerUris::new(
        matches.value_of(ARG_SELF_URL).unwrap(),
        matches.value_of(ARG_UPSTREAM_URL),
    )?;

    let RepoConfigs {
        metaconfig: _,
        repos,
        common,
    } = args::read_configs(&matches)?;

    let futs = repos
        .into_iter()
        .filter(|(_name, config)| config.enabled)
        .map(|(name, config)| {
            open_blobrepo(
                config.storage_config.clone(),
                RepositoryId::new(config.repoid),
                myrouter_port,
                caching,
                config.bookmarks_cache_ttl,
                config.redaction,
                common.scuba_censored_table.clone(),
                config.filestore.clone(),
                logger.clone(),
            )
            .compat()
            .map(|repo| repo.map(|repo| (name, repo)))
        });

    let mut runtime = tokio::runtime::Runtime::new()?;

    let repos: HashMap<_, _> = runtime
        .block_on(try_join_all(futs).compat())?
        .into_iter()
        .collect();

    let root = router(LfsServerContext::new(logger.clone(), repos, server)?);
    let addr = format!("{}:{}", listen_host, listen_port);

    let addr = addr
        .to_socket_addrs()
        .chain_err(err_msg("Invalid Listener Address"))?
        .next()
        .ok_or(err_msg("Invalid Socket Address"))?;

    let listener = TcpListener::bind(&addr).chain_err(err_msg("Could not start TCP listener"))?;

    match (tls_certificate, tls_private_key, tls_ca, tls_ticket_seeds) {
        (Some(tls_certificate), Some(tls_private_key), Some(tls_ca), tls_ticket_seeds) => {
            let config = secure_utils::SslConfig {
                cert: tls_certificate.to_string(),
                private_key: tls_private_key.to_string(),
                ca_pem: tls_ca.to_string(),
            };

            let tls_ticket_seeds = tls_ticket_seeds
                .unwrap_or(secure_utils::fb_tls::SEED_PATH)
                .to_string();

            let tls_builder = secure_utils::build_tls_acceptor_builder(config.clone())?;
            let fbs_tls_builder = secure_utils::fb_tls::tls_acceptor_builder(
                logger.clone(),
                config.clone(),
                tls_builder,
                tls_ticket_seeds,
            )?;
            let acceptor = fbs_tls_builder.build();

            let server = bind_server(listener, root, move |socket| {
                acceptor.accept_async(socket).map_err({
                    let logger = logger.clone();
                    move |e| {
                        warn!(&logger, "TLS handshake failed: {:?}", e);
                        ()
                    }
                })
            });

            runtime
                .block_on(server)
                .map_err(|()| err_msg("Server failed"))?;
        }
        (None, None, None, None) => {
            let server = bind_server(listener, root, |socket| Ok(socket).into_future());

            runtime
                .block_on(server)
                .map_err(|()| err_msg("Server failed"))?;
        }
        _ => return Err(err_msg("TLS flags must be passed together")),
    }

    std::mem::drop(log_guard);

    Ok(())
}
