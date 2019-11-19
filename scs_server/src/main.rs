/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(unused)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{value_t, Arg};
use cloned::cloned;
use cmdlib::args;
use failure_ext::{err_msg, Error, ResultExt};
use fb303::server::make_FacebookService_server;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use futures::{future, sync, Future};
use futures_preview::{FutureExt, TryFutureExt};
use metaconfig_parser::RepoConfigs;
use mononoke_api::Mononoke;
use panichandler::Fate;
use scuba_ext::ScubaSampleBuilder;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::info;
use source_control::server::make_SourceControlService_server;
use srserver::service_framework::{
    BuildModule, Fb303Module, ProfileModule, ServiceFramework, ThriftStatsModule,
};
use srserver::{ThriftServer, ThriftServerBuilder};
use stats::schedule_stats_aggregation;
use tokio::runtime::Runtime;

mod facebook;
mod source_control_impl;

const ARG_PORT: &str = "port";
const ARG_HOST: &str = "host";
const ARG_SCUBA_DATASET: &str = "scuba-dataset";
const ARG_SHUTDOWN_GRACE_PERIOD: &str = "shutdown-grace-period";

const SERVICE_NAME: &str = "mononoke_scs_server";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    panichandler::set_panichandler(Fate::Abort);

    let matches = args::MononokeApp::new("Mononoke Source Control Service Server")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .arg(
            Arg::with_name(ARG_HOST)
                .short("H")
                .long("host")
                .takes_value(true)
                .default_value("::")
                .value_name("HOST")
                .help("Thrift port"),
        )
        .arg(
            Arg::with_name(ARG_PORT)
                .short("p")
                .long("port")
                .default_value("8367")
                .value_name("PORT")
                .help("Thrift port"),
        )
        .arg(
            Arg::with_name(ARG_SCUBA_DATASET)
                .long("scuba-dataset")
                .takes_value(true)
                .help("The name of the scuba dataset to log to"),
        )
        .arg(
            Arg::with_name(ARG_SHUTDOWN_GRACE_PERIOD)
                .long("shutdown-grace-period")
                .takes_value(true)
                .required(false)
                .default_value("0"),
        )
        .get_matches();

    let logger = args::init_logging(fb, &matches);
    let caching = args::init_cachelib(fb, &matches);
    let port = value_t!(matches.value_of(ARG_PORT), u16)?;
    let host = matches.value_of(ARG_HOST).unwrap_or("::");
    let config_path = matches
        .value_of("mononoke-config-path")
        .expect("must set config path");

    let stats_aggregation = schedule_stats_aggregation()
        .expect("failed to create stats aggregation scheduler")
        .map_err(Error::from);

    let mut runtime = Runtime::new().expect("failed to create tokio runtime");
    let exec = runtime.executor();

    let repo_configs = RepoConfigs::read_configs(config_path)?;

    let mut scuba_builder = if let Some(scuba_dataset) = matches.value_of(ARG_SCUBA_DATASET) {
        ScubaSampleBuilder::new(fb, scuba_dataset)
    } else {
        ScubaSampleBuilder::with_discard()
    };

    scuba_builder.add_common_server_data();

    let mononoke = Arc::new(
        runtime.block_on(
            Mononoke::new(
                fb,
                logger.clone(),
                repo_configs,
                args::parse_myrouter_port(&matches),
                caching,
                args::parse_readonly_storage(&matches),
            )
            .boxed()
            .compat(),
        )?,
    );

    let will_exit = Arc::new(AtomicBool::new(false));

    // Initialize the FB303 Thrift stack.

    let fb303_base = {
        cloned!(will_exit);
        move |proto| {
            make_BaseService_server(proto, facebook::BaseServiceImpl::new(will_exit.clone()))
        }
    };
    let fb303 = move |proto| {
        make_FacebookService_server(proto, facebook::FacebookServiceImpl, fb303_base.clone())
    };
    let service = {
        cloned!(logger);
        move |proto| {
            make_SourceControlService_server(
                proto,
                source_control_impl::SourceControlServiceImpl::new(
                    fb,
                    mononoke.clone(),
                    logger.clone(),
                    scuba_builder.clone(),
                ),
                fb303.clone(),
            )
        }
    };

    let thrift: ThriftServer = ThriftServerBuilder::new(fb)
        .with_name(SERVICE_NAME)
        .expect("failed to set name")
        .with_address(&host, port.into(), false)?
        .with_tls()
        .expect("failed to enable TLS")
        .with_factory(exec, move || service)
        .build();

    let mut service_framework = ServiceFramework::from_server(SERVICE_NAME, thrift, port as u32)
        .context("Failed to create service framework server")?;

    service_framework.add_module(BuildModule)?;
    service_framework.add_module(ThriftStatsModule)?;
    service_framework.add_module(Fb303Module)?;
    service_framework.add_module(ProfileModule)?;

    let (sender, receiver) = sync::oneshot::channel::<()>();
    let main = stats_aggregation.select2(receiver).then({
        cloned!(logger);
        move |res| -> Result<(), ()> {
            if let Ok(future::Either::B(_)) = res {
                info!(logger, "Shut down server signalled");
            } else {
                // NOTE: We need to panic here, because otherwise main is going to be blocked on
                // waiting for a signal forever. This shouldn't normally ever happen.
                unreachable!("Server terminated or signal listener was dropped.")
            }

            Ok(())
        }
    });

    // Start listening.
    info!(logger, "Listening on {}:{}", &host, port);
    service_framework
        .serve_background()
        .expect("failed to start thrift service");
    runtime.spawn(main);

    // Wait for a signal that tells us to exit.
    // TODO(mbthomas): This pattern is copied from LFS server, move to cmdlib.
    let signals = Signals::new(&[SIGTERM, SIGINT])?;
    for signal in signals.forever() {
        info!(logger, "Signalled: {}", signal);
        break;
    }

    // Shutting down: wait for the grace period.
    let shutdown_grace_period: u64 = matches
        .value_of(ARG_SHUTDOWN_GRACE_PERIOD)
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    info!(
        logger,
        "Waiting {}s before shutting down server", shutdown_grace_period,
    );
    will_exit.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_secs(shutdown_grace_period));

    // Shut down.
    info!(logger, "Shutting down server...");
    drop(service_framework);
    let _ = sender.send(());

    // Mononoke uses `tokio::spawn` to start background futures that never
    // complete. Set a timeout to abort the server after 10 seconds.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10));
        panic!("Timed out shutting down runtime");
    });

    // Wait for requests to finish
    info!(logger, "Waiting for in-flight requests to finish...");
    runtime
        .shutdown_on_idle()
        .wait()
        .map_err(|_| err_msg("Failed to shutdown runtime!"))?;

    info!(logger, "Exiting...");
    Ok(())
}
