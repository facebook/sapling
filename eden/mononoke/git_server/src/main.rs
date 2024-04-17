/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]
#![feature(never_type)]
#![feature(let_chains)]

use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::Bookmarks;
use bookmarks_cache::BookmarksCache;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use cloned::cloned;
use cmdlib_caching::CachelibSettings;
use commit_graph::CommitGraph;
use connection_security_checker::ConnectionSecurityChecker;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::BookmarkCacheOptions;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::future::try_select;
use futures::pin_mut;
use futures::TryFutureExt;
use git_symbolic_refs::GitSymbolicRefs;
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
use gotham_ext::serve;
use http::HeaderValue;
use metaconfig_types::RepoConfig;
use metaconfig_types::ShardedService;
use mononoke_app::args::McrouterAppExtension;
use mononoke_app::args::ReadonlyArgs;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::args::TLSArgs;
use mononoke_app::args::WarmBookmarksCacheExtension;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::MononokeReposManager;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_permission_checker::RepoPermissionChecker;
use slog::info;
use tokio::net::TcpListener;

use crate::middleware::OdsMiddleware;
use crate::middleware::RequestContentEncodingMiddleware;
use crate::middleware::ResponseContentTypeMiddleware;
use crate::model::GitServerContext;
use crate::scuba::MononokeGitScubaHandler;
use crate::service::build_router;
use crate::sharding::MononokeGitServerProcess;
mod command;

mod errors;
mod middleware;
mod model;
mod read;
mod scuba;
mod service;
mod sharding;
mod util;

const SERVICE_NAME: &str = "mononoke_git_server";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 60;

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
    repo_blobstore: RepoBlobstore,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    git_symbolic_refs: dyn GitSymbolicRefs,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    repo_permission_checker: dyn RepoPermissionChecker,

    #[facet]
    pub warm_bookmarks_cache: dyn BookmarksCache,
}

/// Mononoke Git Server
#[derive(Parser)]
struct GitServerArgs {
    /// Shutdown timeout args for this service
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    /// TLS parameters for this service
    #[clap(flatten)]
    tls_params: Option<TLSArgs>,
    /// The host to listen on locally
    #[clap(long, default_value = "127.0.0.1")]
    listen_host: String,
    /// The port to listen on locally
    #[clap(long, default_value = "8001")]
    listen_port: String,
    // Use this for debugging with tcpdump.
    // Note that this compromises the secrecy of TLS sessions.
    #[clap(long)]
    tls_session_data_log_file: Option<String>,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<String>,
    /// Args for sharding of repos on Mononoke Git Server
    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,
    /// Flag determining if the server should be read-only
    #[clap(flatten)]
    readonly: ReadonlyArgs,
    /// Flag determining if the server should skip enforcing authorization
    #[clap(long)]
    skip_authorization: bool,
    /// Whether or not to use test-friendly logging
    #[clap(long)]
    test_friendly_logging: bool,
}

#[derive(Clone)]
pub struct GitRepos {
    pub(crate) repo_mgr: Arc<MononokeReposManager<Repo>>,
}

impl GitRepos {
    pub(crate) async fn new(repo_mgr: Arc<MononokeReposManager<Repo>>) -> Result<Self> {
        Ok(Self { repo_mgr })
    }

    pub(crate) fn get(&self, repo_name: &str) -> Option<Arc<Repo>> {
        self.repo_mgr.repos().get_by_name(repo_name)
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let cachelib_settings = CachelibSettings {
        expected_item_size_bytes: Some(CACHE_OBJECT_SIZE),
        ..Default::default()
    };

    let app = MononokeAppBuilder::new(fb)
        .with_default_scuba_dataset("mononoke_git_server")
        .with_bookmarks_cache(BookmarkCacheOptions {
            cache_kind: BookmarkCacheKind::Local,
            derived_data: BookmarkCacheDerivedData::GitOnly,
        })
        .with_app_extension(WarmBookmarksCacheExtension {})
        .with_app_extension(McrouterAppExtension {})
        .with_app_extension(Fb303AppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .with_cachelib_settings(cachelib_settings)
        .build::<GitServerArgs>()?;

    let args: GitServerArgs = app.args()?;
    let logger = app.logger().clone();

    let listen_host = args.listen_host.clone();
    let listen_port = args.listen_port.clone();
    let bound_addr_path = args.bound_address_file.clone();

    let addr = format!("{}:{}", listen_host, listen_port);

    let tls_acceptor = args
        .tls_params
        .map(|tls_params| {
            secure_utils::SslConfig::new(
                tls_params.tls_ca,
                tls_params.tls_certificate,
                tls_params.tls_private_key,
                tls_params.tls_ticket_seeds,
            )
            .build_tls_acceptor(logger.clone())
        })
        .transpose()?;
    let acl_provider = app.environment().acl_provider.clone();
    let mut scuba = app.environment().scuba_sample_builder.clone();
    let common = app.repo_configs().common.clone();
    let tls_session_data_log = args.tls_session_data_log_file.clone();
    let enforce_authorization = !args.skip_authorization;
    let log_middleware = if args.test_friendly_logging {
        LogMiddleware::test_friendly()
    } else {
        LogMiddleware::slog(logger.clone())
    };
    let will_exit = Arc::new(AtomicBool::new(false));
    let runtime = app.runtime().clone();
    // Service name is used for shallow or deep sharding. If sharding itself is disabled, provide
    // service name as None while opening repos.
    let service_name = args
        .sharded_executor_args
        .sharded_service_name
        .as_ref()
        .map(|_| ShardedService::MononokeGitServer);
    app.start_monitoring(SERVICE_NAME, AliveService)?;
    app.start_stats_aggregation()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = {
        cloned!(logger, will_exit);
        move |app: MononokeApp| async move {
            let repos_mgr = Arc::new(app.open_managed_repos(service_name).await?);
            let repos = GitRepos::new(repos_mgr.clone())
                .await
                .context(Error::msg("Error opening repos"))?;
            let configs = repos.repo_mgr.configs();

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
            let git_server_context =
                GitServerContext::new(repos, enforce_authorization, logger.clone());

            let router = build_router(git_server_context);

            let capture_session_data = tls_session_data_log.is_some();

            let handler = MononokeHttpHandler::builder()
                .add(TlsSessionDataMiddleware::new(tls_session_data_log)?)
                .add(ServerIdentityMiddleware::new(HeaderValue::from_static(
                    "mononoke_git_server",
                )))
                .add(MetadataMiddleware::new(
                    fb,
                    logger.clone(),
                    common.internal_identity.clone(),
                    ClientEntryPoint::MononokeGitServer,
                ))
                .add(RequestContentEncodingMiddleware {})
                .add(RequestContextMiddleware::new(
                    fb,
                    logger.clone(),
                    scuba.clone(),
                    None,
                    args.readonly.readonly,
                ))
                .add(ResponseContentTypeMiddleware {})
                .add(PostResponseMiddleware::default())
                .add(LoadMiddleware::new())
                .add(log_middleware)
                .add(OdsMiddleware::new())
                .add(<ScubaMiddleware<MononokeGitScubaHandler>>::new({
                    scuba.add("log_tag", "MononokeGit Request Processed");
                    scuba
                }))
                .add(TimerMiddleware::new())
                .add(ConfigInfoMiddleware::new(configs))
                .build(router);

            info!(&logger, "Listening on {}", bound_addr);
            // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
            if let Some(bound_addr_path) = bound_addr_path {
                let mut writer = File::create(bound_addr_path)?;
                writer.write_all(bound_addr.as_bytes())?;
                writer.write_all(b"\n")?;
            }

            if let Some(mut executor) = args.sharded_executor_args.build_executor(
                app.fb,
                runtime.clone(),
                &logger,
                || Arc::new(MononokeGitServerProcess::new(repos_mgr)),
                false, // disable shard (repo) level healing
                SM_CLEANUP_TIMEOUT_SECS,
            )? {
                // The Sharded Process Executor needs to branch off and execute
                // on its own dedicated task spawned off the common tokio runtime.
                runtime.spawn({
                    let logger = app.logger().clone();
                    {
                        cloned!(will_exit);
                        async move { executor.block_and_execute(&logger, will_exit).await }
                    }
                });
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
