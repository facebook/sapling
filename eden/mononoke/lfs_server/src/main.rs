/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]
#![feature(never_type)]
#![deny(warnings)]

use anyhow::{anyhow, bail, Context, Error};
use clap::{Arg, Values};
use cloned::cloned;
use fbinit::FacebookInit;
use futures::{
    channel::oneshot,
    future::{lazy, select, try_join_all},
    FutureExt, TryFutureExt,
};
use futures_util::try_join;
use gotham::{bind_server, bind_server_with_socket_data};
use gotham_ext::{handler::MononokeHttpHandler, socket_data::TlsSocketData};
use hyper::header::HeaderValue;
use permission_checker::{ArcPermissionChecker, MononokeIdentitySet, PermissionCheckerBuilder};
use slog::{info, warn};
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use tokio::net::TcpListener;

use blobrepo::BlobRepo;
use blobrepo_factory::BlobrepoBuilder;
use cmdlib::{
    args::{self, get_config_handle},
    helpers::serve_forever,
    monitoring::{start_fb303_server, AliveService},
};
use metaconfig_parser::RepoConfigs;

use crate::lfs_server_context::{LfsServerContext, ServerUris};
use crate::middleware::{
    ClientIdentityMiddleware, LoadMiddleware, LogMiddleware, OdsMiddleware,
    RequestContextMiddleware, ScubaMiddleware, ServerIdentityMiddleware, TimerMiddleware,
    TlsSessionDataMiddleware,
};
use crate::service::build_router;

mod batch;
mod config;
mod download;
mod errors;
mod lfs_server_context;
mod middleware;
mod service;
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
const ARG_ALWAYS_WAIT_FOR_UPSTREAM: &str = "always-wait-for-upstream";
const ARG_LIVE_CONFIG: &str = "live-config";
const ARG_LIVE_CONFIG_FETCH_INTERVAL: &str = "live-config-fetch-interval";
const ARG_TRUSTED_PROXY_IDENTITY: &str = "trusted-proxy-identity";
const ARG_TEST_IDENTITY: &str = "allowed-test-identity";
const ARG_TEST_FRIENDLY_LOGGING: &str = "test-friendly-logging";
const ARG_TLS_SESSION_DATA_LOG_FILE: &str = "tls-session-data-log-file";
const ARG_MAX_UPLOAD_SIZE: &str = "max-upload-size";
const ARG_DISABLE_ACL_CHECKER: &str = "disable-acl-checker";

const SERVICE_NAME: &str = "mononoke_lfs_server";

// Used to determine how many entries are in cachelib's HashTable. A smaller
// object size results in more entries and possibly higher idle memory usage.
// More info: https://fburl.com/wiki/i78i3uzk
const CACHE_OBJECT_SIZE: usize = 256 * 1024;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke LFS Server")
        .with_advanced_args_hidden()
        .with_all_repos()
        .with_shutdown_timeout_args()
        .with_scuba_logging_args()
        .with_fb303_args()
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
            Arg::with_name(ARG_ALWAYS_WAIT_FOR_UPSTREAM)
                .long(ARG_ALWAYS_WAIT_FOR_UPSTREAM)
                .takes_value(false)
                .help(
                    "Whether to always wait for an upstream response (primarily useful in testing)",
                ),
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
            Arg::with_name(ARG_TEST_IDENTITY)
                .long(ARG_TEST_IDENTITY)
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Test identity to allow (NOTE: this will disable AclChecker)"),
        )
        .arg(
            Arg::with_name(ARG_TEST_FRIENDLY_LOGGING)
                .long(ARG_TEST_FRIENDLY_LOGGING)
                .takes_value(false)
                .required(false)
                .help("Whether or not to use test-friendly logging"),
        )
        .arg(
            Arg::with_name(ARG_TLS_SESSION_DATA_LOG_FILE)
                .takes_value(true)
                .required(false)
                .help(
                    "A file to which to log TLS session data, including master secrets. \
                     Use this for debugging with tcpdump. \
                     Note that this compromises the secrecy of TLS sessions.",
                )
                .long(ARG_TLS_SESSION_DATA_LOG_FILE),
        )
        .arg(
            Arg::with_name(ARG_MAX_UPLOAD_SIZE)
                .long(ARG_MAX_UPLOAD_SIZE)
                .takes_value(true)
                .required(false)
                .help("A limit (in bytes) to enforce for uploads."),
        )
        .arg(
            Arg::with_name(ARG_DISABLE_ACL_CHECKER)
                .long(ARG_DISABLE_ACL_CHECKER)
                .takes_value(false)
                .required(false)
                .help("Whether to disable ACL checks (only use this locally!)"),
        );

    let matches = app.get_matches();

    let (caching, logger, mut runtime) =
        args::init_mononoke(fb, &matches, Some(CACHE_OBJECT_SIZE))?;

    let mysql_options = args::parse_mysql_options(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();

    let tls_certificate = matches.value_of(ARG_TLS_CERTIFICATE);
    let tls_private_key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let tls_ca = matches.value_of(ARG_TLS_CA);
    let tls_ticket_seeds = matches.value_of(ARG_TLS_TICKET_SEEDS);

    let tls_session_data_log = matches.value_of(ARG_TLS_SESSION_DATA_LOG_FILE);

    let mut scuba_logger = args::get_scuba_sample_builder(fb, &matches)?;

    let trusted_proxy_idents = idents_from_values(matches.values_of(ARG_TRUSTED_PROXY_IDENTITY))?;

    scuba_logger.add_common_server_data();

    let test_idents = idents_from_values(matches.values_of(ARG_TEST_IDENTITY))?;
    let disable_acl_checker = matches.is_present(ARG_DISABLE_ACL_CHECKER);

    let test_acl_checker = if !test_idents.is_empty() {
        Some(ArcPermissionChecker::from(
            PermissionCheckerBuilder::allowlist_checker(test_idents),
        ))
    } else {
        None
    };

    let server = ServerUris::new(
        matches.value_of(ARG_SELF_URL).unwrap(),
        matches.value_of(ARG_UPSTREAM_URL),
    )?;

    let RepoConfigs { repos, common } = args::load_repo_configs(fb, &matches)?;

    let futs = repos
        .into_iter()
        .filter(|(_name, config)| config.enabled)
        .map(|(name, config)| {
            let scuba_censored_table = common.scuba_censored_table.clone();
            cloned!(blobstore_options, test_acl_checker, logger);
            async move {
                let builder = BlobrepoBuilder::new(
                    fb,
                    name.clone(),
                    &config,
                    mysql_options,
                    caching,
                    scuba_censored_table,
                    readonly_storage,
                    blobstore_options,
                    &logger,
                );

                let hipster_acl = config.hipster_acl;
                let aclchecker = async {
                    if let Some(test_checker) = test_acl_checker {
                        Ok(test_checker.clone())
                    } else {
                        Ok(ArcPermissionChecker::from(
                            match (disable_acl_checker, hipster_acl) {
                                (true, _) | (false, None) => {
                                    PermissionCheckerBuilder::always_allow()
                                }
                                (_, Some(acl)) => {
                                    info!(
                                        logger,
                                        "{}: Actions will be checked against {} ACL", name, acl
                                    );
                                    PermissionCheckerBuilder::acl_for_repo(fb, &acl).await?
                                }
                            },
                        ))
                    }
                };

                let (repo, aclchecker) = try_join!(builder.build(), aclchecker)?;

                Result::<(String, (BlobRepo, ArcPermissionChecker)), Error>::Ok((
                    name,
                    (repo, aclchecker),
                ))
            }
        });

    let repos: HashMap<_, _> = runtime
        .block_on_std(try_join_all(futs))?
        .into_iter()
        .collect();

    let will_exit = Arc::new(AtomicBool::new(false));

    let config_interval: u64 = matches
        .value_of(ARG_LIVE_CONFIG_FETCH_INTERVAL)
        .unwrap()
        .parse()?;

    let config_handle = get_config_handle(
        fb,
        logger.clone(),
        matches.value_of(ARG_LIVE_CONFIG),
        config_interval,
    )
    .context(Error::msg("Failed to load configuration"))?;

    let max_upload_size: Option<u64> = matches
        .value_of(ARG_MAX_UPLOAD_SIZE)
        .map(|u| u.parse())
        .transpose()?;

    let ctx = LfsServerContext::new(
        repos,
        server,
        matches.is_present(ARG_ALWAYS_WAIT_FOR_UPSTREAM),
        max_upload_size,
        will_exit.clone(),
        config_handle.clone(),
    )?;

    let log_middleware = match matches.is_present(ARG_TEST_FRIENDLY_LOGGING) {
        true => LogMiddleware::test_friendly(),
        false => LogMiddleware::slog(logger.clone()),
    };

    let router = build_router(fb, ctx);

    let root = MononokeHttpHandler::builder()
        .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
        .add(ClientIdentityMiddleware::new(trusted_proxy_idents))
        .add(RequestContextMiddleware::new(
            fb,
            logger.clone(),
            config_handle,
        ))
        .add(LoadMiddleware::new())
        .add(log_middleware)
        .add(ServerIdentityMiddleware::new(HeaderValue::from_static(
            "mononoke-lfs",
        )))
        .add(ScubaMiddleware::new(scuba_logger))
        .add(OdsMiddleware::new())
        .add(TimerMiddleware::new())
        .build(router);

    let addr = format!("{}:{}", listen_host, listen_port);

    let addr = addr
        .to_socket_addrs()
        .context(Error::msg("Invalid Listener Address"))?
        .next()
        .ok_or(Error::msg("Invalid Socket Address"))?;

    start_fb303_server(fb, SERVICE_NAME, &logger, &matches, AliveService)?;

    let listener = runtime
        .block_on_std(TcpListener::bind(&addr))
        .context(Error::msg("Could not start TCP listener"))?;

    let server = match (tls_certificate, tls_private_key, tls_ca, tls_ticket_seeds) {
        (Some(tls_certificate), Some(tls_private_key), Some(tls_ca), tls_ticket_seeds) => {
            let acceptor = Arc::new(
                secure_utils::SslConfig::new(
                    tls_ca,
                    tls_certificate,
                    tls_private_key,
                    tls_ticket_seeds,
                )
                .build_tls_acceptor(logger.clone())?,
            );

            let capture_session_data = tls_session_data_log.is_some();

            bind_server_with_socket_data(listener, root, {
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
        (None, None, None, None) => {
            bind_server(listener, root, |socket| async move { Ok(socket) }).right_future()
        }
        _ => bail!("TLS flags must be passed together"),
    };

    info!(&logger, "Listening on {:?}", addr);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    serve_forever(
        runtime,
        select(
            server.boxed().map_err(|()| anyhow!("Unhandled error")),
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

    info!(&logger, "Exiting...");
    Ok(())
}

fn idents_from_values(matches: Option<Values>) -> Result<MononokeIdentitySet, Error> {
    match matches {
        Some(matches) => matches.map(FromStr::from_str).collect(),
        None => Ok(MononokeIdentitySet::new()),
    }
}
