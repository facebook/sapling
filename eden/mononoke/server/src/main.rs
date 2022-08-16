/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use cache_warmup::cache_warmup;
use clap::Parser;
use cloned::cloned;
use cmdlib_logging::ScribeLoggingArgs;
use environment::WarmBookmarksCacheDerivedData;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_watchdog::WatchdogExt;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::McrouterAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::fb303::ReadyFlagService;
use mononoke_app::MononokeAppBuilder;
use openssl::ssl::AlpnError;
use slog::error;
use slog::info;
use slog::o;

/// Mononoke Server
#[derive(Parser)]
struct MononokeServerArgs {
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// TCP address to listen to in format `host:port
    #[clap(long)]
    listening_host_port: String,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<PathBuf>,
    /// If provided the thrift server will start on this port
    #[clap(long, short = 'p')]
    thrift_port: Option<String>,
    /// Path to a file with server certificate
    #[clap(long)]
    cert: String,
    /// Path to a file with server private key
    #[clap(long)]
    private_key: String,
    /// Path to a file with CA certificate
    #[clap(long)]
    ca_pem: String,
    /// Path to a file with SCS client certificate
    #[clap(long)]
    scs_client_cert: Option<String>,
    /// Path to a file with SCS client private key
    #[clap(long, requires = "scs-client-cert")]
    scs_client_private_key: Option<String>,
    /// Path to a file with encryption keys for SSL tickets
    #[clap(long)]
    ssl_ticket_seeds: Option<String>,
    /// Top level Mononoke tier where CSLB publishes routing table
    #[clap(long)]
    cslb_config: Option<String>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = Arc::new(
        MononokeAppBuilder::new(fb)
            .with_default_scuba_dataset("mononoke_test_perf")
            .with_warm_bookmarks_cache(WarmBookmarksCacheDerivedData::HgOnly)
            .with_app_extension(McrouterAppExtension {})
            .with_app_extension(Fb303AppExtension {})
            .with_app_extension(HooksAppExtension {})
            .with_app_extension(RepoFilterAppExtension {})
            .build::<MononokeServerArgs>()?,
    );
    let args: MononokeServerArgs = app.args()?;

    let root_log = app.logger();
    let runtime = app.runtime();

    let cslb_config = args.cslb_config.clone();
    info!(root_log, "Starting up");

    #[cfg(fbcode_build)]
    if let (Some(cert_path), Some(key_path)) = (&args.scs_client_cert, &args.scs_client_private_key)
    {
        pushrebase_client::override_certificate_paths(cert_path, key_path, &args.ca_pem);
    }

    let configs = app.repo_configs().clone();

    let acceptor = {
        let mut builder = secure_utils::SslConfig::new(
            args.ca_pem,
            args.cert,
            args.private_key,
            args.ssl_ticket_seeds,
        )
        .tls_acceptor_builder(root_log.clone())
        .context("Failed to instantiate TLS Acceptor builder")?;

        builder.set_alpn_select_callback(|_, protos| {
            // NOTE: Currently we do not support HTTP/2 here yet.
            alpn::alpn_select(protos, alpn::HGCLI_ALPN)
                .map_err(|_| AlpnError::ALERT_FATAL)?
                .ok_or(AlpnError::NOACK)
        });

        builder.build()
    };

    info!(root_log, "Creating repo listeners");

    let service = ReadyFlagService::new();
    let (terminate_sender, terminate_receiver) = oneshot::channel::<()>();

    let scribe = args.scribe_logging_args.get_scribe(fb)?;
    let host_port = args.listening_host_port;

    let bound_addr_file = args.bound_address_file;

    let env = app.environment();

    let scuba = env.scuba_sample_builder.clone();

    let will_exit = Arc::new(AtomicBool::new(false));

    let repo_listeners = {
        cloned!(root_log, service, will_exit, env);
        let app = Arc::clone(&app);
        async move {
            let common = configs.common.clone();
            let mononoke = Arc::new(Mononoke::new(Arc::clone(&app)).watched(&root_log).await?);
            info!(&root_log, "Built Mononoke");

            info!(&root_log, "Warming up cache");
            stream::iter(mononoke.repos())
                .map(|repo| {
                    let repo_name = repo.name().to_string();
                    let blob_repo = repo.blob_repo().clone();
                    let root_log = root_log.clone();
                    let cache_warmup_params = repo.config().cache_warmup.clone();
                    async move {
                        let logger = root_log.new(o!("repo" => repo_name.clone()));
                        let ctx = CoreContext::new_with_logger(fb, logger);
                        cache_warmup(&ctx, &blob_repo, cache_warmup_params)
                            .await
                            .with_context(|| {
                                format!("Error while warming up cache for repo {}", repo_name)
                            })
                    }
                })
                // Repo cache warmup can be quite expensive, let's limit to 40
                // at a time.
                .buffer_unordered(40)
                .try_collect()
                .await?;
            info!(&root_log, "Cache warmup completed");
            repo_listener::create_repo_listeners(
                fb,
                common,
                mononoke.clone(),
                root_log,
                host_port,
                acceptor,
                service,
                terminate_receiver,
                &env.config_store,
                scribe,
                &scuba,
                will_exit,
                cslb_config,
                bound_addr_file,
                env.acl_provider.as_ref(),
            )
            .await
        }
    };

    // Thread with a thrift service is now detached
    let fb303_args = app.extension_args::<Fb303AppExtension>()?;
    fb303_args.start_fb303_server(fb, "mononoke_server", root_log, service)?;

    cmdlib::helpers::serve_forever(
        runtime,
        repo_listeners,
        root_log,
        move || will_exit.store(true, Ordering::Relaxed),
        args.shutdown_timeout_args.shutdown_grace_period,
        async {
            match terminate_sender.send(()) {
                Err(err) => error!(root_log, "could not send termination signal: {:?}", err),
                _ => {}
            }
            repo_listener::wait_for_connections_closed(root_log).await;
        },
        args.shutdown_timeout_args.shutdown_timeout,
    )
}
