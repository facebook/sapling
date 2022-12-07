/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use cloned::cloned;
use cmdlib_logging::ScribeLoggingArgs;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use land_service_if::server::*;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::MononokeAppBuilder;
use slog::info;
use srserver::service_framework::BuildModule;
use srserver::service_framework::Fb303Module;
use srserver::service_framework::ServiceFramework;
use srserver::service_framework::ThriftStatsModule;
use srserver::ThriftServer;
use srserver::ThriftServerBuilder;
use tokio::task;
use LandService_metadata_sys::create_metadata;

const SERVICE_NAME: &str = "mononoke_land_service";

mod conversion_helpers;
mod errors;
mod facebook;
mod factory;
mod land_changeset_object;
mod land_service_impl;
mod scuba_request;
mod scuba_response;
mod worker;

#[derive(Debug, Parser)]
struct LandServiceServerArgs {
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// Thrift host
    #[clap(long, short = 'H', default_value = "::")]
    host: String,
    /// Thrift port
    #[clap(long, short = 'p', default_value_t = 8485)]
    port: u16,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<String>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb).build::<LandServiceServerArgs>()?;

    // Process commandline flags
    let args: LandServiceServerArgs = app.args()?;

    let logger = app.logger().clone();
    let runtime = app.runtime();
    let exec = runtime.clone();
    let env = app.environment();

    let scuba_builder = env.scuba_sample_builder.clone();
    let mononoke = Arc::new(
        runtime
            .block_on(app.open_managed_repos())?
            .make_mononoke_api()?,
    );

    let will_exit = Arc::new(AtomicBool::new(false));

    let fb303_base = {
        cloned!(will_exit);
        move |proto| {
            make_BaseService_server(proto, facebook::BaseServiceImpl::new(will_exit.clone()))
        }
    };

    let factory = factory::Factory::new(
        fb,
        logger.clone(),
        mononoke,
        scuba_builder,
        args.scribe_logging_args.get_scribe(fb)?,
        &app.repo_configs().common,
    );

    let land_service_server = land_service_impl::LandServiceImpl::new(factory);

    let service = {
        move |proto| {
            make_LandService_server(
                proto,
                land_service_server.thrift_server(),
                fb303_base.clone(),
            )
        }
    };

    let thrift: ThriftServer = ThriftServerBuilder::new(fb)
        .with_name(SERVICE_NAME)
        .expect("failed to set name")
        .with_address(&args.host, args.port, false)?
        .with_tls()
        .expect("failed to enable TLS")
        .with_cancel_if_client_disconnected()
        .with_metadata(create_metadata())
        .with_factory(exec, move || service)
        .build();

    let mut service_framework = ServiceFramework::from_server(SERVICE_NAME, thrift)
        .context("Failed to create service framework server")?;

    service_framework.add_module(BuildModule)?;
    service_framework.add_module(ThriftStatsModule)?;
    service_framework.add_module(Fb303Module)?;

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

    // Monitoring is provided by the `Fb303Module`, but we must still start
    // stats aggregation.
    app.start_stats_aggregation()?;

    app.wait_until_terminated(
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
