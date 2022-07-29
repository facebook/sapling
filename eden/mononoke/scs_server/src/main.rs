/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![feature(bool_to_option)]
#![deny(unused)]
#![type_length_limit = "2097152"]

use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use clap::Parser;
use cloned::cloned;
use cmdlib::helpers::serve_forever;
use cmdlib_logging::ScribeLoggingArgs;
use connection_security_checker::ConnectionSecurityChecker;
use environment::WarmBookmarksCacheDerivedData;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use megarepo_api::MegarepoApi;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::MononokeAppBuilder;
use panichandler::Fate;
use permission_checker::DefaultAclProvider;
use slog::info;
use source_control::server::make_SourceControlService_server;
use srserver::service_framework::BuildModule;
use srserver::service_framework::ContextPropModule;
use srserver::service_framework::Fb303Module;
use srserver::service_framework::ProfileModule;
use srserver::service_framework::ServiceFramework;
use srserver::service_framework::ThriftStatsModule;
use srserver::ThriftServer;
use srserver::ThriftServerBuilder;
use tokio::task;

mod commit_id;
mod errors;
mod facebook;
mod from_request;
mod history;
mod into_response;
mod metadata;
mod methods;
mod monitoring;
mod scuba_common;
mod scuba_params;
mod scuba_response;
mod source_control_impl;
mod specifiers;

const SERVICE_NAME: &str = "mononoke_scs_server";

/// Mononoke Source Control Service Server
#[derive(Parser)]
struct ScsServerArgs {
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// Thrift host
    #[clap(long, short = 'H', default_value = "::")]
    host: String,
    /// Thrift port
    #[clap(long, short = 'p', default_value_t = 8367)]
    port: u16,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<String>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    panichandler::set_panichandler(Fate::Abort);

    let app = Arc::new(
        MononokeAppBuilder::new(fb)
            .with_warm_bookmarks_cache(WarmBookmarksCacheDerivedData::AllKinds)
            .with_app_extension(HooksAppExtension {})
            .with_app_extension(RepoFilterAppExtension {})
            .build::<ScsServerArgs>()?,
    );

    let args: ScsServerArgs = app.args()?;

    let logger = app.logger();
    let runtime = app.runtime();

    let exec = runtime.clone();
    let env = app.environment();

    let scuba_builder = env.scuba_sample_builder.clone();
    let repo_factory = app.repo_factory();

    let mononoke = Arc::new(runtime.block_on(Mononoke::new(Arc::clone(&app)))?);
    let megarepo_api = Arc::new(runtime.block_on(MegarepoApi::new(
        env,
        app.repo_configs().clone(),
        repo_factory,
        mononoke.clone(),
    ))?);

    let will_exit = Arc::new(AtomicBool::new(false));

    // Initialize the FB303 Thrift stack.

    let fb303_base = {
        cloned!(will_exit);
        move |proto| {
            make_BaseService_server(proto, facebook::BaseServiceImpl::new(will_exit.clone()))
        }
    };
    let acl_provider = DefaultAclProvider::new(fb);
    let security_checker = runtime.block_on(ConnectionSecurityChecker::new(
        acl_provider,
        &app.repo_configs().common,
    ))?;
    let source_control_server = source_control_impl::SourceControlServiceImpl::new(
        fb,
        mononoke.clone(),
        megarepo_api,
        logger.clone(),
        scuba_builder,
        args.scribe_logging_args.get_scribe(fb)?,
        security_checker,
        &app.repo_configs().common,
    );
    let service = {
        move |proto| {
            make_SourceControlService_server(
                proto,
                source_control_server.thrift_server(),
                fb303_base.clone(),
            )
        }
    };

    let monitoring_forever = {
        let monitoring_ctx = CoreContext::new_with_logger(fb, logger.clone());
        monitoring::monitoring_stats_submitter(monitoring_ctx, mononoke)
    };

    let thrift: ThriftServer = ThriftServerBuilder::new(fb)
        .with_name(SERVICE_NAME)
        .expect("failed to set name")
        .with_address(&args.host, args.port, false)?
        .with_tls()
        .expect("failed to enable TLS")
        .with_cancel_if_client_disconnected()
        .with_metadata(metadata::create_metadata())
        .with_factory(exec, move || service)
        .build();

    let mut service_framework = ServiceFramework::from_server(SERVICE_NAME, thrift)
        .context("Failed to create service framework server")?;

    service_framework.add_module(BuildModule)?;
    service_framework.add_module(ThriftStatsModule)?;
    service_framework.add_module(Fb303Module)?;
    service_framework.add_module(ProfileModule)?;
    service_framework.add_module(ContextPropModule)?;

    service_framework
        .serve_background()
        .expect("failed to start thrift service");

    let bound_addr = format!(
        "{}:{}",
        &args.host,
        service_framework.get_address()?.get_port()?
    );
    info!(logger, "Listening on {}", bound_addr);

    // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
    if let Some(bound_addr_path) = args.bound_address_file {
        let mut writer = File::create(bound_addr_path)?;
        writer.write_all(bound_addr.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    serve_forever(
        runtime,
        monitoring_forever.map(Result::<(), Error>::Ok),
        logger,
        move || will_exit.store(true, Ordering::Relaxed),
        args.shutdown_timeout_args.shutdown_grace_period,
        async {
            // Note that async blocks are lazy, so this isn't called until first poll
            let _ = task::spawn_blocking(move || {
                // Calling `stop` blocks until the service has completed all requests.
                service_framework.stop();
            })
            .await;
        },
        args.shutdown_timeout_args.shutdown_timeout,
    )?;

    info!(logger, "Exiting...");
    Ok(())
}
