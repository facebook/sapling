/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![recursion_limit = "256"]
#![feature(async_closure, option_flattening, never_type)]
#![deny(warnings)]

use clap::Arg;
use failure::{err_msg, Error};
use fbinit::FacebookInit;
use futures::{empty, future::Either, sync::oneshot, Future, IntoFuture};
use futures_ext::FutureExt as Futures01Ext;
use futures_preview::{FutureExt, TryFutureExt};
use futures_util::{compat::Future01CompatExt, try_future::try_join_all};
use gotham::bind_server;
use scuba::ScubaSampleBuilder;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{info, warn};
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_openssl::SslAcceptorExt;

use blobrepo_factory::open_blobrepo;
use failure_ext::chain::ChainExt;
use metaconfig_parser::RepoConfigs;

use cmdlib::{args, monitoring::create_fb303_and_stats_agg};

use crate::config::spawn_config_poller;
use crate::handler::MononokeLfsHandler;
use crate::lfs_server_context::{LfsServerContext, ServerUris};
use crate::middleware::{
    ClientIdentityMiddleware, LoadMiddleware, LogMiddleware, OdsMiddleware,
    RequestContextMiddleware, ScubaMiddleware, ServerIdentityMiddleware, TimerMiddleware,
};
use crate::router::build_router;

mod batch;
mod config;
mod download;
mod errors;
mod handler;
mod lfs_server_context;
mod middleware;
mod router;
mod upload;
#[macro_use]
mod http;

const ARG_SELF_URL: &str = "self-url";
const ARG_UPSTREAM_URL: &str = "upstream-url";
const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";
const ARG_TLS_CERTIFICATE: &str = "tls-certificate";
const ARG_TLS_PRIVATE_KEY: &str = "tls-private-key";
const ARG_TLS_CA: &str = "tls-ca";
const ARG_TLS_TICKET_SEEDS: &str = "tls-ticket-seeds";
const ARG_SCUBA_DATASET: &str = "scuba-dataset";
const ARG_ALWAYS_WAIT_FOR_UPSTREAM: &str = "always-wait-for-upstream";
const ARG_SHUTDOWN_GRACE_PERIOD: &str = "shutdown-grace-period";
const ARG_SCUBA_LOG_FILE: &str = "scuba-log-file";
const ARG_LIVE_CONFIG: &str = "live-config";
const ARG_LIVE_CONFIG_FETCH_INTERVAL: &str = "live-config-fetch-interval";

const SERVICE_NAME: &str = "mononoke_lfs_server";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke LFS Server")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
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
        )
        .arg(
            Arg::with_name(ARG_SCUBA_DATASET)
                .long(ARG_SCUBA_DATASET)
                .takes_value(true)
                .help("The name of the scuba dataset to log to"),
        )
        .arg(
            Arg::with_name(ARG_ALWAYS_WAIT_FOR_UPSTREAM)
                .long(ARG_ALWAYS_WAIT_FOR_UPSTREAM)
                .takes_value(false)
                .help(
                    "Whether to always wait for an upstream response (primarily useful in testing)",
                ),
        )
        .arg(
            Arg::with_name(ARG_SHUTDOWN_GRACE_PERIOD)
                .long("shutdown-grace-period")
                .takes_value(true)
                .required(false)
                .default_value("0"),
        )
        .arg(
            Arg::with_name(ARG_SCUBA_LOG_FILE)
                .long(ARG_SCUBA_LOG_FILE)
                .takes_value(true)
                .help("A log file to write Scuba logs to (primarily useful in testing)"),
        )
        .arg(
            Arg::with_name(ARG_LIVE_CONFIG)
                .long(ARG_LIVE_CONFIG)
                .takes_value(true)
                .required(false)
                .help("Source for live config (configerator:SPEC, file:SPEC, default)"),
        )
        .arg(
            Arg::with_name(ARG_LIVE_CONFIG_FETCH_INTERVAL)
                .long(ARG_LIVE_CONFIG_FETCH_INTERVAL)
                .takes_value(true)
                .required(false)
                .default_value("5")
                .help("How often to reload the live config, in seconds"),
        );

    let app = args::add_fb303_args(app);

    let matches = app.get_matches();

    let caching = args::init_cachelib(fb, &matches);
    let logger = args::init_logging(fb, &matches);
    let myrouter_port = args::parse_myrouter_port(&matches);

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();

    let tls_certificate = matches.value_of(ARG_TLS_CERTIFICATE);
    let tls_private_key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let tls_ca = matches.value_of(ARG_TLS_CA);
    let tls_ticket_seeds = matches.value_of(ARG_TLS_TICKET_SEEDS);

    let mut scuba_logger = if let Some(scuba_dataset) = matches.value_of(ARG_SCUBA_DATASET) {
        ScubaSampleBuilder::new(fb, scuba_dataset)
    } else {
        ScubaSampleBuilder::with_discard()
    };

    scuba_logger.add_common_server_data();

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
                fb,
                config.storage_config.clone(),
                config.repoid,
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

    let stats_aggregation = match create_fb303_and_stats_agg(fb, SERVICE_NAME, &logger, &matches)? {
        Some(fut) => fut.discard().left_future(),
        None => empty().right_future(),
    };

    let repos: HashMap<_, _> = runtime
        .block_on(try_join_all(futs).compat())?
        .into_iter()
        .collect();

    let will_exit = Arc::new(AtomicBool::new(false));

    let config_interval: u64 = matches
        .value_of(ARG_LIVE_CONFIG_FETCH_INTERVAL)
        .unwrap()
        .parse()?;

    let (poller, config) = spawn_config_poller(
        fb,
        logger.clone(),
        will_exit.clone(),
        matches.value_of(ARG_LIVE_CONFIG),
        config_interval,
    )
    .chain_err(err_msg("Failed to load configuration"))?;

    let ctx = LfsServerContext::new(
        fb,
        logger.clone(),
        repos,
        server,
        matches.is_present(ARG_ALWAYS_WAIT_FOR_UPSTREAM),
        will_exit.clone(),
        config,
    )?;

    let router = build_router(ctx);

    let root = MononokeLfsHandler::builder()
        .add(ClientIdentityMiddleware::new())
        .add(RequestContextMiddleware::new())
        .add(LogMiddleware::new(logger.clone()))
        .add(ServerIdentityMiddleware::new())
        .add(LoadMiddleware::new())
        .add(ScubaMiddleware::new(
            scuba_logger,
            matches.value_of(ARG_SCUBA_LOG_FILE),
        )?)
        .add(OdsMiddleware::new())
        .add(TimerMiddleware::new())
        .build(router);

    let addr = format!("{}:{}", listen_host, listen_port);

    let addr = addr
        .to_socket_addrs()
        .chain_err(err_msg("Invalid Listener Address"))?
        .next()
        .ok_or(err_msg("Invalid Socket Address"))?;

    let listener = TcpListener::bind(&addr).chain_err(err_msg("Could not start TCP listener"))?;

    let run_server = match (tls_certificate, tls_private_key, tls_ca, tls_ticket_seeds) {
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

            bind_server(listener, root, {
                let logger = logger.clone();
                move |socket| {
                    acceptor.accept_async(socket).map_err({
                        let logger = logger.clone();
                        move |e| {
                            warn!(&logger, "TLS handshake failed: {:?}", e);
                            ()
                        }
                    })
                }
            })
            .left_future()
        }
        (None, None, None, None) => {
            bind_server(listener, root, |socket| Ok(socket).into_future()).right_future()
        }
        _ => return Err(err_msg("TLS flags must be passed together")),
    };

    let (sender, receiver) = oneshot::channel::<()>();
    let main = run_server.join(stats_aggregation).select2(receiver).then({
        let logger = logger.clone();
        move |res| -> Result<(), ()> {
            if let Ok(Either::B(_)) = res {
                // We were signalled.
                info!(&logger, "Shut down server");
            } else {
                // NOTE: We need to panic here, because otherwise main is going to be blocked on
                // waiting for a signal forever. This shouldn't normally ever happen.
                unreachable!("Server terminated or signal listener was dropped.")
            }

            Ok(())
        }
    });

    // Start listening.
    info!(&logger, "Listening on {:?}", addr);
    runtime.spawn(main);

    // Wait for a signal that tells us to exit.
    let signals = Signals::new(&[SIGTERM, SIGINT])?;
    for signal in signals.forever() {
        info!(&logger, "Signalled: {}", signal);
        break;
    }

    // Report unhealthy
    let shutdown_grace_period: u64 = matches
        .value_of(ARG_SHUTDOWN_GRACE_PERIOD)
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    info!(
        &logger,
        "Waiting {}s before shutting down server", shutdown_grace_period,
    );
    will_exit.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_secs(shutdown_grace_period));

    info!(&logger, "Shutting down server...");
    let _ = sender.send(());

    // Wait for requests to finish.
    info!(&logger, "Waiting for in-flight requests to finish...");
    runtime
        .shutdown_on_idle()
        .wait()
        .map_err(|_| err_msg("Failed to shutdown runtime!"))?;

    info!(&logger, "Waiting for configuration poller to exit...");
    poller
        .join()
        .map_err(|_| err_msg("Failed to shutdown configuration poller!"))?;

    info!(&logger, "Exiting...");

    Ok(())
}
