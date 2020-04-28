/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgMatches};
use cloned::cloned;
use futures::{
    channel::oneshot,
    future::{lazy, select, FutureExt, TryFutureExt},
};
use gotham::{bind_server, bind_server_with_socket_data};
use hyper::header::HeaderValue;
use openssl::ssl::SslAcceptor;
use slog::{debug, info, warn, Logger};
use tokio::net::TcpListener;

use aclchecker::Identity;
use cmdlib::{
    args,
    helpers::serve_forever,
    monitoring::{start_fb303_server, AliveService},
};
use fbinit::FacebookInit;
use gotham_ext::{
    handler::MononokeHttpHandler,
    middleware::{ClientIdentityMiddleware, ServerIdentityMiddleware, TlsSessionDataMiddleware},
    socket_data::TlsSocketData,
};
use mononoke_api::Mononoke;
use secure_utils::SslConfig;

mod context;
mod handlers;
mod middleware;

use crate::context::EdenApiServerContext;
use crate::handlers::build_router;
use crate::middleware::RequestContextMiddleware;

const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";
const ARG_TLS_CERTIFICATE: &str = "tls-certificate";
const ARG_TLS_PRIVATE_KEY: &str = "tls-private-key";
const ARG_TLS_CA: &str = "tls-ca";
const ARG_TLS_TICKET_SEEDS: &str = "tls-ticket-seeds";
const ARG_TRUSTED_PROXY_IDENTITY: &str = "trusted-proxy-identity";
const ARG_TLS_SESSION_DATA_LOG_FILE: &str = "tls-session-data-log-file";

const SERVICE_NAME: &str = "mononoke_edenapi_server";

const DEFAULT_HOST: &str = "::";
const DEFAULT_PORT: &str = "8000";

/// Get the IP address and port the server should listen on.
fn parse_server_addr(matches: &ArgMatches) -> Result<SocketAddr> {
    let host = matches
        .value_of(ARG_LISTEN_HOST)
        .unwrap_or(DEFAULT_HOST)
        .parse()
        .context("Invalid IP address specified")?;
    let port = matches
        .value_of(ARG_LISTEN_PORT)
        .unwrap_or(DEFAULT_PORT)
        .parse()
        .context("Invalid port specified")?;
    Ok(SocketAddr::new(host, port))
}

/// Read the command line arguments related to TLS credentials.
fn parse_tls_options(matches: &ArgMatches) -> Option<(SslConfig, String)> {
    let cert = matches.value_of(ARG_TLS_CERTIFICATE);
    let key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let ca = matches.value_of(ARG_TLS_CA);
    let ticket_seeds = matches
        .value_of(ARG_TLS_TICKET_SEEDS)
        .unwrap_or(secure_utils::fb_tls::SEED_PATH)
        .to_string();

    cert.and_then(|cert| {
        key.and_then(|key| {
            ca.map(|ca| {
                let ssl_config = SslConfig {
                    ca_pem: ca.to_string(),
                    cert: cert.to_string(),
                    private_key: key.to_string(),
                };
                (ssl_config, ticket_seeds)
            })
        })
    })
}

/// Create and configure an `SslAcceptor` that can accept and decrypt TLS
/// connections, accounting for FB-specific TLS configuration.
fn build_tls_acceptor(
    config: SslConfig,
    ticket_seeds: String,
    logger: &Logger,
) -> Result<SslAcceptor> {
    // Create an async acceptor that handles the TLS handshake and decryption.
    let builder = secure_utils::build_tls_acceptor_builder(config.clone())?;

    // Configure the acceptor to work with FB's expected TLS setup.
    let builder = secure_utils::fb_tls::tls_acceptor_builder(
        logger.clone(),
        config.clone(),
        builder,
        ticket_seeds,
    )?;

    Ok(builder.build())
}

/// Parse AclChecker identities passed in as arguments.
fn parse_identities(matches: &ArgMatches) -> Result<Vec<Identity>> {
    match matches.values_of(ARG_TRUSTED_PROXY_IDENTITY) {
        Some(values) => values.map(FromStr::from_str).collect(),
        None => Ok(Vec::new()),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = args::MononokeApp::new("EdenAPI Server")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_all_repos()
        .with_shutdown_timeout_args()
        .build()
        .arg(
            Arg::with_name(ARG_LISTEN_HOST)
                .long(ARG_LISTEN_HOST)
                .takes_value(true)
                .default_value(DEFAULT_HOST)
                .help("The host to listen on locally"),
        )
        .arg(
            Arg::with_name(ARG_LISTEN_PORT)
                .long(ARG_LISTEN_PORT)
                .takes_value(true)
                .default_value(DEFAULT_PORT)
                .help("The port to listen on locally"),
        )
        .arg(
            Arg::with_name(ARG_TLS_CERTIFICATE)
                .long(ARG_TLS_CERTIFICATE)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_TLS_PRIVATE_KEY)
                .long(ARG_TLS_PRIVATE_KEY)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_TLS_CA)
                .long(ARG_TLS_CA)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_TLS_TICKET_SEEDS)
                .long(ARG_TLS_TICKET_SEEDS)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_TRUSTED_PROXY_IDENTITY)
                .long(ARG_TRUSTED_PROXY_IDENTITY)
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Proxy identity to trust"),
        )
        .arg(
            Arg::with_name(ARG_TLS_SESSION_DATA_LOG_FILE)
                .long(ARG_TLS_SESSION_DATA_LOG_FILE)
                .takes_value(true)
                .required(false)
                .help(
                    "A file to which to log TLS session data, including master secrets. \
                     Use this for debugging with tcpdump. \
                     Note that this compromises the secrecy of TLS sessions.",
                ),
        );

    let matches = app.get_matches();

    let (caching, logger, mut runtime) = args::init_mononoke(fb, &matches, None)?;

    debug!(logger, "Reading args");
    let repo_configs = args::read_configs(fb, &matches)?;
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let trusted_proxy_idents = parse_identities(&matches)?;
    let tls_session_data_log = matches.value_of(ARG_TLS_SESSION_DATA_LOG_FILE);

    debug!(logger, "Initializing Mononoke API");
    let mononoke = runtime.block_on(
        Mononoke::new(
            fb,
            logger.clone(),
            repo_configs,
            mysql_options,
            caching,
            readonly_storage,
            blobstore_options,
        )
        .boxed()
        .compat(),
    )?;

    // Global flag that the main loop will set to True when the server
    // has been signalled to gracefully shut down.
    let will_exit = Arc::new(AtomicBool::new(false));

    // Set up context to hold the server's global state.
    let ctx = EdenApiServerContext::new(mononoke, will_exit.clone());

    // Set up the router and handler for serving HTTP requests, along with custom middleware.
    // The middleware added here does not implement Gotham's usual Middleware trait; instead,
    // it uses the custom Middleware API defined in the gotham_ext crate. Native Gotham
    // middleware is set up during router setup in build_router.
    let router = build_router(ctx);
    let handler = MononokeHttpHandler::builder()
        .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
        .add(ClientIdentityMiddleware::new(trusted_proxy_idents))
        .add(ServerIdentityMiddleware::new(HeaderValue::from_static(
            "edenapi_server",
        )))
        .add(RequestContextMiddleware::new(fb, logger.clone()))
        .build(router);

    // Set up socket and TLS acceptor that this server will listen on.
    let addr = parse_server_addr(&matches)?;
    let listener = runtime.block_on_std(TcpListener::bind(&addr))?;
    let acceptor = parse_tls_options(&matches)
        .map(|(config, ticket_seeds)| build_tls_acceptor(config, ticket_seeds, &logger))
        .transpose()?;

    // Bind to the socket and set up the Future for the server's main loop.
    let scheme = if acceptor.is_some() { "https" } else { "http" };
    let server = match acceptor {
        Some(acceptor) => {
            let acceptor = Arc::new(acceptor);
            let capture_session_data = tls_session_data_log.is_some();

            bind_server_with_socket_data(listener, handler, {
                cloned!(logger);
                move |socket| {
                    cloned!(acceptor, logger);
                    async move {
                        let ssl_socket = match tokio_openssl::accept(&acceptor, socket).await {
                            Ok(ssl_socket) => ssl_socket,
                            Err(e) => {
                                warn!(&logger, "TLS handshake failed: {:?}", e);
                                return Err(());
                            }
                        };

                        let socket_data =
                            TlsSocketData::from_ssl(ssl_socket.ssl(), capture_session_data);

                        Ok((socket_data, ssl_socket))
                    }
                }
            })
            .left_future()
        }
        None => bind_server(listener, handler, |socket| async move { Ok(socket) }).right_future(),
    };

    // Spawn a basic FB303 Thrift server for stats reporting.
    start_fb303_server(fb, SERVICE_NAME, &logger, &matches, AliveService)?;

    // Start up the HTTP server on the Tokio runtime.
    info!(logger, "Listening for requests at {}://{}", scheme, addr);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    serve_forever(
        runtime,
        select(
            server.boxed().map_err(|()| anyhow!("unexpected error")),
            shutdown_rx.map_err(|err| anyhow!("Cancelled channel: {}", err)),
        )
        .map(|res| res.factor_first().0),
        &logger,
        move || will_exit.store(true, Ordering::Relaxed),
        args::get_shutdown_grace_period(&matches)?,
        lazy(move |_| {
            let _ = shutdown_tx.send(());
            // Currently we kill off in-flight requests as soon as we've closed the listener.
            // If this is a problem in prod, this would be the point at which to wait
            // for all connections to shut down.
            // To do this properly, we'd need to track the `Connection` futures that Gotham
            // gets from Hyper, tell them to gracefully shutdown, then wait for them to complete
        }),
        args::get_shutdown_timeout(&matches)?,
    )?;

    info!(logger, "Exiting...");
    Ok(())
}
