/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::net::SocketAddr;

use anyhow::{Context, Result};
use clap::{Arg, ArgMatches};
use fbinit::FacebookInit;
use futures::future;
use futures_ext::FutureExt;
use gotham::{bind_server, state::State};
use openssl::ssl::SslAcceptor;
use slog::Logger;
use tokio::{net::TcpListener, prelude::*};
use tokio_openssl::SslAcceptorExt;

use cmdlib::args;
use secure_utils::SslConfig;

const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";
const ARG_TLS_CERTIFICATE: &str = "tls-certificate";
const ARG_TLS_PRIVATE_KEY: &str = "tls-private-key";
const ARG_TLS_CA: &str = "tls-ca";
const ARG_TLS_TICKET_SEEDS: &str = "tls-ticket-seeds";

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

fn say_hello(state: State) -> (State, &'static str) {
    (state, "Hello, world!\n")
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
            || Ok(say_hello),
            move |socket| acceptor.accept_async(socket).map_err(|_| ()),
        )
        .left_future(),
        None => bind_server(listener, || Ok(say_hello), |socket| future::ok(socket)).right_future(),
    };

    slog::info!(logger, "Listening for requests at {}://{}", scheme, addr);
    tokio::run(server);

    Ok(())
}
