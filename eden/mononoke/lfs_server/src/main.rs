/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]
#![feature(never_type)]
#![deny(warnings)]

use anyhow::{anyhow, bail, Context, Error};
use cached_config::ConfigHandle;
use clap::{Arg, Values};
use cloned::cloned;
use fbinit::FacebookInit;
use futures::{
    channel::oneshot,
    future::{lazy, select, try_join_all},
    FutureExt, TryFutureExt,
};
use futures_util::try_join;
use gotham_ext::{
    handler::MononokeHttpHandler,
    middleware::{
        ClientIdentityMiddleware, LoadMiddleware, LogMiddleware, PostResponseMiddleware,
        ScubaMiddleware, ServerIdentityMiddleware, TimerMiddleware, TlsSessionDataMiddleware,
    },
    serve,
};
use hyper::header::HeaderValue;
use permission_checker::{ArcPermissionChecker, MononokeIdentitySet, PermissionCheckerBuilder};
use slog::info;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use tokio::net::TcpListener;

use blobrepo::BlobRepo;
use cmdlib::{
    args::{self, parse_config_spec_to_path, CachelibSettings},
    helpers::serve_forever,
    monitoring::{start_fb303_server, AliveService},
};
use metaconfig_parser::RepoConfigs;
use metaconfig_types::RepoConfig;
use repo_factory::RepoFactory;

use crate::lfs_server_context::{LfsServerContext, ServerUris};
use crate::middleware::{OdsMiddleware, RequestContextMiddleware};
use crate::scuba::LfsScubaHandler;
use crate::service::build_router;

mod batch;
mod config;
mod download;
mod errors;
mod lfs_server_context;
mod middleware;
mod popularity;
mod scuba;
mod service;
mod upload;
mod util;

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
    let cachelib_settings = CachelibSettings {
        expected_item_size_bytes: Some(CACHE_OBJECT_SIZE),
        ..Default::default()
    };

    let app = args::MononokeAppBuilder::new("Mononoke LFS Server")
        .with_cachelib_settings(cachelib_settings.clone())
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
                .value_delimiter(",")
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
                .help("Path to config in configerator"),
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

    let matches = app.get_matches(fb)?;


    let logger = matches.logger();
    let runtime = matches.runtime();
    let config_store = matches.config_store();

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();

    let tls_certificate = matches.value_of(ARG_TLS_CERTIFICATE);
    let tls_private_key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let tls_ca = matches.value_of(ARG_TLS_CA);
    let tls_ticket_seeds = matches.value_of(ARG_TLS_TICKET_SEEDS);

    let tls_session_data_log = matches.value_of(ARG_TLS_SESSION_DATA_LOG_FILE);

    let scuba_logger = matches.scuba_sample_builder();

    let trusted_proxy_idents = idents_from_values(matches.values_of(ARG_TRUSTED_PROXY_IDENTITY))?;

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
        matches.values_of(ARG_SELF_URL).unwrap().collect(),
        matches.value_of(ARG_UPSTREAM_URL),
    )?;

    let RepoConfigs { repos, common } = args::load_repo_configs(config_store, &matches)?;

    let repo_factory = Arc::new(RepoFactory::new(matches.environment().clone(), &common));

    let futs = repos
        .into_iter()
        .filter(|(_name, config)| config.enabled)
        .map(|(name, config)| {
            cloned!(repo_factory, test_acl_checker, logger);
            async move {
                let repo = repo_factory
                    .build(name.clone(), config.clone())
                    .map_err(Error::from);

                let hipster_acl = config.hipster_acl.as_ref();
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

                let (repo, aclchecker) = try_join!(repo, aclchecker)?;

                Result::<(String, (BlobRepo, ArcPermissionChecker, RepoConfig)), Error>::Ok((
                    name,
                    (repo, aclchecker, config),
                ))
            }
        });

    let repos: HashMap<_, _> = runtime.block_on(try_join_all(futs))?.into_iter().collect();

    let will_exit = Arc::new(AtomicBool::new(false));

    let config_handle = match matches.value_of(ARG_LIVE_CONFIG) {
        Some(spec) => config_store.get_config_handle_DEPRECATED(parse_config_spec_to_path(spec)?),
        None => Ok(ConfigHandle::default()),
    };

    let config_handle = config_handle.context(Error::msg("Failed to load configuration"))?;

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

    let handler = MononokeHttpHandler::builder()
        .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
        .add(ClientIdentityMiddleware::new())
        .add(PostResponseMiddleware::with_config(config_handle))
        .add(RequestContextMiddleware::new(fb, logger.clone()))
        .add(LoadMiddleware::new())
        .add(log_middleware)
        .add(ServerIdentityMiddleware::new(HeaderValue::from_static(
            "mononoke-lfs",
        )))
        .add(<ScubaMiddleware<LfsScubaHandler>>::new(scuba_logger))
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
        .block_on(TcpListener::bind(&addr))
        .context(Error::msg("Could not start TCP listener"))?;

    let server = match (tls_certificate, tls_private_key, tls_ca, tls_ticket_seeds) {
        (Some(tls_certificate), Some(tls_private_key), Some(tls_ca), tls_ticket_seeds) => {
            let acceptor = secure_utils::SslConfig::new(
                tls_ca,
                tls_certificate,
                tls_private_key,
                tls_ticket_seeds,
            )
            .build_tls_acceptor(logger.clone())?;

            let capture_session_data = tls_session_data_log.is_some();

            serve::https(
                logger.clone(),
                listener,
                acceptor,
                capture_session_data,
                trusted_proxy_idents,
                handler,
            )
            .left_future()
        }
        (None, None, None, None) => serve::http(logger.clone(), listener, handler).right_future(),
        _ => bail!("TLS flags must be passed together"),
    };

    info!(&logger, "Listening on {:?}", addr);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    serve_forever(
        runtime,
        select(
            server.boxed(),
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
