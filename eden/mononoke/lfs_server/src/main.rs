/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]
#![feature(never_type)]

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use cached_config::ConfigHandle;
use clap::Arg;
use clap::Values;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::future::lazy;
use futures::future::select;
use futures::future::try_join_all;
use futures::FutureExt;
use futures::TryFutureExt;
use gotham_ext::handler::MononokeHttpHandler;
use gotham_ext::middleware::ClientIdentityMiddleware;
use gotham_ext::middleware::LoadMiddleware;
use gotham_ext::middleware::LogMiddleware;
use gotham_ext::middleware::PostResponseMiddleware;
use gotham_ext::middleware::ScubaMiddleware;
use gotham_ext::middleware::ServerIdentityMiddleware;
use gotham_ext::middleware::TimerMiddleware;
use gotham_ext::middleware::TlsSessionDataMiddleware;
use gotham_ext::serve;
use hyper::header::HeaderValue;
use permission_checker::ArcPermissionChecker;
use permission_checker::MononokeIdentitySet;
use permission_checker::PermissionCheckerBuilder;
use slog::info;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::net::TcpListener;

use cmdlib::args;
use cmdlib::args::CachelibSettings;
use cmdlib::helpers::serve_forever;
use cmdlib::monitoring::start_fb303_server;
use cmdlib::monitoring::AliveService;
use filestore::FilestoreConfig;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::RepoConfig;
use mononoke_app::args::parse_config_spec_to_path;
use repo_blobstore::RepoBlobstore;
use repo_factory::RepoFactory;
use repo_identity::RepoIdentity;

use crate::lfs_server_context::LfsServerContext;
use crate::lfs_server_context::ServerUris;
use crate::middleware::OdsMiddleware;
use crate::middleware::RequestContextMiddleware;
use crate::scuba::LfsScubaHandler;
use crate::service::build_router;

mod batch;
mod config;
mod download;
mod errors;
mod git_upload;
mod lfs_server_context;
mod middleware;
mod popularity;
mod scuba;
mod service;
mod upload;
mod util;

const ARG_BOUND_ADDR_FILE: &str = "bound-address-file";
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
const ARG_GIT_BLOB_UPLOAD_ALLOWED: &str = "git-blob-upload-allowed";

const SERVICE_NAME: &str = "mononoke_lfs_server";

// Used to determine how many entries are in cachelib's HashTable. A smaller
// object size results in more entries and possibly higher idle memory usage.
// More info: https://fburl.com/wiki/i78i3uzk
const CACHE_OBJECT_SIZE: usize = 256 * 1024;

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[init(repo_identity.name().to_string())]
    name: String,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let cachelib_settings = CachelibSettings {
        expected_item_size_bytes: Some(CACHE_OBJECT_SIZE),
        ..Default::default()
    };

    let app = args::MononokeAppBuilder::new("Mononoke LFS Server")
        .with_cachelib_settings(cachelib_settings)
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
            Arg::with_name(ARG_BOUND_ADDR_FILE)
                .long(ARG_BOUND_ADDR_FILE)
                .required(false)
                .takes_value(true)
                .help("path for file in which to write the bound tcp address in rust std::net::SocketAddr format"),
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
                .required_unless(ARG_BOUND_ADDR_FILE)
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
        )
        .arg(
            Arg::with_name(ARG_GIT_BLOB_UPLOAD_ALLOWED)
                .long(ARG_GIT_BLOB_UPLOAD_ALLOWED)
                .takes_value(false)
                .required(false)
                .help("Whether to enable Mononoke-specific small git blob uploads")
        );

    let matches = app.get_matches(fb)?;

    let logger = matches.logger().clone();
    let runtime = matches.runtime();
    let config_store = matches.config_store();

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap().to_string();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();
    let bound_addr_path = matches.value_of(ARG_BOUND_ADDR_FILE).map(|v| v.to_string());

    let git_blob_upload_allowed = matches.is_present(ARG_GIT_BLOB_UPLOAD_ALLOWED);

    let addr = format!("{}:{}", listen_host, listen_port);

    let tls_certificate = matches.value_of(ARG_TLS_CERTIFICATE);
    let tls_private_key = matches.value_of(ARG_TLS_PRIVATE_KEY);
    let tls_ca = matches.value_of(ARG_TLS_CA);
    let tls_ticket_seeds = matches.value_of(ARG_TLS_TICKET_SEEDS);

    let tls_acceptor = match (tls_certificate, tls_private_key, tls_ca, tls_ticket_seeds) {
        (Some(tls_certificate), Some(tls_private_key), Some(tls_ca), tls_ticket_seeds) => {
            let acceptor = secure_utils::SslConfig::new(
                tls_ca,
                tls_certificate,
                tls_private_key,
                tls_ticket_seeds,
            )
            .build_tls_acceptor(logger.clone())?;
            Some(acceptor)
        }
        (None, None, None, None) => None,
        _ => bail!("TLS flags must be passed together"),
    };

    let tls_session_data_log = matches
        .value_of(ARG_TLS_SESSION_DATA_LOG_FILE)
        .map(|v| v.to_string());

    let scuba_logger = matches.scuba_sample_builder();

    let trusted_proxy_idents = idents_from_values(matches.values_of(ARG_TRUSTED_PROXY_IDENTITY))?;

    let test_idents = idents_from_values(matches.values_of(ARG_TEST_IDENTITY))?;
    let disable_acl_checker = matches.is_present(ARG_DISABLE_ACL_CHECKER);

    let test_acl_checker = if !test_idents.is_empty() {
        Some(ArcPermissionChecker::from(
            PermissionCheckerBuilder::new()
                .allow_allowlist(test_idents)
                .build(),
        ))
    } else {
        None
    };

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

    let self_urls: Option<Vec<String>> = matches
        .values_of(ARG_SELF_URL)
        .map(|v| v.into_iter().map(|v| v.to_string()).collect());
    let upstream_url = matches.value_of(ARG_UPSTREAM_URL).map(|v| v.to_string());
    let always_wait_for_upstream = matches.is_present(ARG_ALWAYS_WAIT_FOR_UPSTREAM);
    let log_middleware = match matches.is_present(ARG_TEST_FRIENDLY_LOGGING) {
        true => LogMiddleware::test_friendly(),
        false => LogMiddleware::slog(logger.clone()),
    };

    let RepoConfigs { repos, common } = args::load_repo_configs(config_store, &matches)?;
    let internal_identity = common.internal_identity.clone();

    let repo_factory = Arc::new(RepoFactory::new(matches.environment().clone(), &common));

    let futs = repos
        .into_iter()
        .filter(|(_name, config)| config.enabled)
        .map({
            cloned!(repo_factory, test_acl_checker, logger);
            move |(name, config)| {
                cloned!(test_acl_checker, logger, repo_factory, config.hipster_acl);
                async move {
                    let aclchecker = if let Some(test_checker) = test_acl_checker {
                        test_checker
                    } else {
                        ArcPermissionChecker::from(match (disable_acl_checker, hipster_acl) {
                            (true, _) | (false, None) => {
                                PermissionCheckerBuilder::new().allow_all().build()
                            }
                            (_, Some(acl)) => {
                                info!(
                                    logger,
                                    "{}: Actions will be checked against {} ACL", name, acl
                                );
                                PermissionCheckerBuilder::new()
                                    .allow(repo_factory.acl_provider().repo_acl(&acl).await?)
                                    .build()
                            }
                        })
                    };

                    let repo = repo_factory.build(name.clone(), config.clone()).await?;

                    Result::<(String, (Repo, ArcPermissionChecker, RepoConfig)), Error>::Ok((
                        name,
                        (repo, aclchecker, config),
                    ))
                }
            }
        });

    let server = {
        cloned!(logger, will_exit);
        async move {
            let repos: HashMap<_, _> = try_join_all(futs).await?.into_iter().collect();

            let addr = addr
                .to_socket_addrs()
                .context(Error::msg("Invalid Listener Address"))?
                .next()
                .ok_or_else(|| Error::msg("Invalid Socket Address"))?;

            let listener = TcpListener::bind(&addr)
                .await
                .context(Error::msg("Could not start TCP listener"))?;

            // We use the listen_host rather than the ip of listener.local_addr()
            // because the certs user passed will be referencing listen_host
            let bound_addr = format!("{}:{}", listen_host, listener.local_addr()?.port());

            // For tests we use one empty string self_url, map it to None
            let self_urls = self_urls.and_then(|self_urls| {
                if self_urls.len() == 1 && self_urls[0].is_empty() {
                    None
                } else {
                    Some(self_urls)
                }
            });

            let self_urls = if let Some(self_urls) = self_urls {
                self_urls
            } else {
                let protocol = if tls_acceptor.is_some() {
                    "https://"
                } else {
                    "http://"
                };
                vec![protocol.to_owned() + &bound_addr]
            };

            let server_uris = ServerUris::new(self_urls, upstream_url)?;

            let ctx = LfsServerContext::new(
                repos,
                server_uris,
                always_wait_for_upstream,
                max_upload_size,
                will_exit,
                config_handle.clone(),
            )?;

            let router = build_router(fb, ctx, git_blob_upload_allowed);

            let capture_session_data = tls_session_data_log.is_some();

            let handler = MononokeHttpHandler::builder()
                .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
                .add(ClientIdentityMiddleware::new(
                    fb,
                    logger.clone(),
                    internal_identity,
                ))
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

            info!(&logger, "Listening on {}", bound_addr);

            // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
            if let Some(bound_addr_path) = bound_addr_path {
                let mut writer = File::create(bound_addr_path)?;
                writer.write_all(bound_addr.as_bytes())?;
                writer.write_all(b"\n")?;
            }

            if let Some(tls_acceptor) = tls_acceptor {
                serve::https(
                    logger,
                    listener,
                    tls_acceptor,
                    capture_session_data,
                    trusted_proxy_idents,
                    handler,
                )
                .await
            } else {
                serve::http(logger, listener, handler).await
            }
        }
    };

    start_fb303_server(fb, SERVICE_NAME, &logger, &matches, AliveService)?;

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
