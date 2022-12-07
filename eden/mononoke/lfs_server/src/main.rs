/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]
#![feature(never_type)]

use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use cached_config::ConfigHandle;
use clap::Parser;
use cloned::cloned;
use cmdlib::args::CachelibSettings;
use connection_security_checker::ConnectionSecurityChecker;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::channel::oneshot;
use futures::future::try_select;
use futures::pin_mut;
use futures::TryFutureExt;
use gotham_ext::handler::MononokeHttpHandler;
use gotham_ext::middleware::LoadMiddleware;
use gotham_ext::middleware::LogMiddleware;
use gotham_ext::middleware::MetadataMiddleware;
use gotham_ext::middleware::PostResponseMiddleware;
use gotham_ext::middleware::ScubaMiddleware;
use gotham_ext::middleware::ServerIdentityMiddleware;
use gotham_ext::middleware::TimerMiddleware;
use gotham_ext::middleware::TlsSessionDataMiddleware;
use gotham_ext::serve;
use hyper::header::HeaderValue;
use metaconfig_types::RepoConfig;
use mononoke_app::args::parse_config_spec_to_path;
use mononoke_app::args::ReadonlyArgs;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::args::TLSArgs;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_repos::MononokeRepos;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;
use repo_permission_checker::RepoPermissionChecker;
use slog::info;
use tokio::net::TcpListener;

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
    repo_config: RepoConfig,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_permission_checker: dyn RepoPermissionChecker,
}

/// Mononoke LFS Server
#[derive(Parser)]
struct LfsServerArgs {
    /// Shutdown timeout args for this service
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    /// TLS parameters for this service
    #[clap(flatten)]
    tls_params: TLSArgs,
    /// The host to listen on locally
    #[clap(long, default_value = "127.0.0.1")]
    listen_host: String,
    /// The port to listen on locally
    #[clap(long, default_value = "8001")]
    listen_port: String,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<String>,
    /// The base URLs for this server
    #[clap(value_delimiter = ',')]
    self_urls: Vec<String>,
    /// The base URL for an upstream server
    #[clap(long)]
    upstream_url: Option<String>,
    /// Whether to always wait for an upstream response (primarily useful in testing)
    #[clap(long)]
    always_wait_for_upstream: bool,
    /// Path to config in configerator
    #[clap(long)]
    live_config: Option<String>,
    /// Whether or not to use test-friendly logging
    #[clap(long)]
    test_friendly_logging: bool,
    // A file to which to log TLS session data, including master secrets.
    // Use this for debugging with tcpdump.
    // Note that this compromises the secrecy of TLS sessions.
    #[clap(long)]
    tls_session_data_log_file: Option<String>,
    /// Whether to enable Mononoke-specific small git blob uploads
    #[clap(long)]
    git_blob_upload_allowed: bool,
    /// A limit (in bytes) to enforce for uploads.
    #[clap(long)]
    max_upload_size: Option<u64>,
    #[clap(flatten)]
    readonly: ReadonlyArgs,
}

#[derive(Clone)]
pub struct LfsRepos {
    pub(crate) repos: Arc<MononokeRepos<Repo>>,
}

impl LfsRepos {
    pub(crate) async fn new(app: &MononokeApp) -> Result<Self> {
        let repos_mgr = app.open_managed_repos().await?;
        let repos = repos_mgr.repos().clone();
        Ok(Self { repos })
    }

    pub(crate) fn get(&self, repo_name: &str) -> Option<Arc<Repo>> {
        self.repos.get_by_name(repo_name)
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let cachelib_settings = CachelibSettings {
        expected_item_size_bytes: Some(CACHE_OBJECT_SIZE),
        ..Default::default()
    };

    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .with_default_cachelib_settings(cachelib_settings)
        .build::<LfsServerArgs>()?;

    let args: LfsServerArgs = app.args()?;

    let logger = app.logger().clone();
    let config_store = app.config_store();
    let acl_provider = app.environment().acl_provider.clone();

    let listen_host = args.listen_host.clone();
    let listen_port = args.listen_port.clone();
    let bound_addr_path = args.bound_address_file.clone();

    let git_blob_upload_allowed = args.git_blob_upload_allowed;

    let addr = format!("{}:{}", listen_host, listen_port);

    let tls_certificate = args.tls_params.tls_certificate.clone();
    let tls_private_key = args.tls_params.tls_private_key.clone();
    let tls_ca = args.tls_params.tls_ca.clone();
    let tls_ticket_seeds = args.tls_params.tls_ticket_seeds.clone();

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

    let tls_session_data_log = args.tls_session_data_log_file.clone();

    let scuba_logger = app.environment().scuba_sample_builder.clone();

    let will_exit = Arc::new(AtomicBool::new(false));

    let config_handle = match &args.live_config {
        Some(spec) => config_store.get_config_handle_DEPRECATED(parse_config_spec_to_path(spec)?),
        None => Ok(ConfigHandle::default()),
    };

    let config_handle = config_handle.context(Error::msg("Failed to load configuration"))?;

    let max_upload_size: Option<u64> = args.max_upload_size;

    let self_urls = args.self_urls;
    let upstream_url = args.upstream_url;
    let always_wait_for_upstream = args.always_wait_for_upstream;
    let log_middleware = if args.test_friendly_logging {
        LogMiddleware::test_friendly()
    } else {
        LogMiddleware::slog(logger.clone())
    };

    app.start_monitoring(SERVICE_NAME, AliveService)?;
    app.start_stats_aggregation()?;

    let common = &app.repo_configs().common;
    let internal_identity = common.internal_identity.clone();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = {
        cloned!(acl_provider, common, logger, will_exit);
        move |app| async move {
            let repos = LfsRepos::new(&app)
                .await
                .context(Error::msg("Error opening repos"))?;

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
            let self_urls =
                if self_urls.is_empty() || (self_urls.len() == 1 && self_urls[0].is_empty()) {
                    None
                } else {
                    Some(self_urls)
                };

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
            let enforce_authentication = ctx.get_config().enforce_authentication();

            let router = build_router(fb, ctx, git_blob_upload_allowed);

            let capture_session_data = tls_session_data_log.is_some();

            let handler = MononokeHttpHandler::builder()
                .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
                .add(MetadataMiddleware::new(
                    fb,
                    logger.clone(),
                    internal_identity,
                ))
                .add(PostResponseMiddleware::with_config(config_handle))
                .add(RequestContextMiddleware::new(
                    fb,
                    logger.clone(),
                    enforce_authentication,
                    args.readonly.readonly,
                ))
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

            let serve = async move {
                if let Some(tls_acceptor) = tls_acceptor {
                    let connection_security_checker =
                        ConnectionSecurityChecker::new(acl_provider.as_ref(), &common).await?;

                    serve::https(
                        logger,
                        listener,
                        tls_acceptor,
                        capture_session_data,
                        connection_security_checker,
                        handler,
                    )
                    .await
                } else {
                    serve::http(logger, listener, handler).await
                }
            };
            pin_mut!(serve);
            try_select(
                serve,
                shutdown_rx.map_err(|err| anyhow!("Cancelled channel: {}", err)),
            )
            .await
            .map_err(|e| futures::future::Either::factor_first(e).0)?;
            Ok(())
        }
    };

    app.run_until_terminated(
        server,
        move || will_exit.store(true, Ordering::Relaxed),
        args.shutdown_timeout_args.shutdown_grace_period,
        async move {
            let _ = shutdown_tx.send(());
            // Currently we kill off in-flight requests as soon as we've closed the listener.
            // If this is a problem in prod, this would be the point at which to wait
            // for all connections to shut down.
            // To do this properly, we'd need to track the `Connection` futures that Gotham
            // gets from Hyper, tell them to gracefully shutdown, then wait for them to complete
        },
        args.shutdown_timeout_args.shutdown_timeout,
    )?;

    info!(&logger, "Exiting...");
    Ok(())
}
