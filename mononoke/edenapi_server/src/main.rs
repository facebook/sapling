/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::net::SocketAddr;

use anyhow::{Context, Result};
use clap::{Arg, ArgMatches};
use fbinit::FacebookInit;
use futures::future;
use futures_ext::FutureExt as OldFutureExt;
use futures_preview::{
    channel::oneshot,
    compat::Future01CompatExt,
    future::{lazy, select, FutureExt, TryFutureExt},
};
use gotham::{bind_server, state::State};
use openssl::ssl::SslAcceptor;
use slog::{error, Logger};
use tokio::{net::TcpListener, prelude::*};
use tokio_openssl::SslAcceptorExt;

use cmdlib::{args, helpers::serve_forever, monitoring::start_fb303_server};
use secure_utils::SslConfig;

const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";
const ARG_TLS_CERTIFICATE: &str = "tls-certificate";
const ARG_TLS_PRIVATE_KEY: &str = "tls-private-key";
const ARG_TLS_CA: &str = "tls-ca";
const ARG_TLS_TICKET_SEEDS: &str = "tls-ticket-seeds";

const SERVICE_NAME: &str = "mononoke_edenapi_server";

const DEFAULT_HOST: &str = "::";
const DEFAULT_PORT: &str = "8000";

/// Get the IP address and port the server should listen on.
fn get_server_addr(matches: &ArgMatches) -> Result<SocketAddr> {
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
fn get_tls_config(matches: &ArgMatches) -> Option<(SslConfig, String)> {
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

fn health_check(state: State) -> (State, &'static str) {
    (state, "I_AM_ALIVE")
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = args::MononokeApp::new("EdenAPI Server")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .arg(
            Arg::with_name(ARG_LISTEN_HOST)
                .long("--listen-host")
                .takes_value(true)
                .default_value(DEFAULT_HOST)
                .help("The host to listen on locally"),
        )
        .arg(
            Arg::with_name(ARG_LISTEN_PORT)
                .long("--listen-port")
                .takes_value(true)
                .default_value(DEFAULT_PORT)
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
        );

    let app = args::add_fb303_args(app);

    let matches = app.get_matches();
    let logger = args::init_logging(fb, &matches);
    let addr = get_server_addr(&matches)?;

    let listener = TcpListener::bind(&addr)?;
    let acceptor = get_tls_config(&matches)
        .map(|(config, ticket_seeds)| build_tls_acceptor(config, ticket_seeds, &logger))
        .transpose()?;

    let scheme = if acceptor.is_some() { "https" } else { "http" };
    let server = match acceptor {
        Some(acceptor) => bind_server(
            listener,
            || Ok(health_check),
            move |socket| acceptor.accept_async(socket).map_err(|_| ()),
        )
        .left_future(),
        None => {
            bind_server(listener, || Ok(health_check), |socket| future::ok(socket)).right_future()
        }
    };

    start_fb303_server(fb, SERVICE_NAME, &logger, &matches)?;

    let runtime = args::init_runtime(&matches)?;

    slog::info!(logger, "Listening for requests at {}://{}", scheme, addr);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    serve_forever(
        runtime,
        select(
            server.compat().map_err({
                let logger = logger.clone();
                move |e| error!(&logger, "Unhandled error: {:?}", e)
            }),
            shutdown_rx,
        )
        .map(|_| ()),
        &logger,
        || {},
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
    )
}
