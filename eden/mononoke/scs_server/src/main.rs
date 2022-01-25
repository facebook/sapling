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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Error};
use clap::{value_t, Arg};
use cloned::cloned;
use cmdlib::{args, helpers::serve_forever};
use fb303::server::make_FacebookService_server;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use megarepo_api::MegarepoApi;
use metaconfig_parser::load_repo_configs;
use mononoke_api::{CoreContext, Mononoke, MononokeApiEnvironment, WarmBookmarksCacheDerivedData};
use panichandler::Fate;
use repo_factory::RepoFactory;
use slog::info;
use source_control::server::make_SourceControlService_server;
use srserver::service_framework::{
    BuildModule, Fb303Module, ProfileModule, ServiceFramework, ThriftStatsModule,
};
use srserver::{ThriftServer, ThriftServerBuilder};
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

const ARG_PORT: &str = "port";
const ARG_HOST: &str = "host";
const ARG_BOUND_ADDR_FILE: &str = "bound-address-file";

const SERVICE_NAME: &str = "mononoke_scs_server";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    panichandler::set_panichandler(Fate::Abort);

    let app = args::MononokeAppBuilder::new("Mononoke Source Control Service Server")
        .with_advanced_args_hidden()
        .with_all_repos()
        .with_shutdown_timeout_args()
        .with_scuba_logging_args()
        .with_disabled_hooks_args()
        .with_scribe_args()
        .build()
        .arg(
            Arg::with_name(ARG_HOST)
                .short("H")
                .long("host")
                .takes_value(true)
                .default_value("::")
                .value_name("HOST")
                .help("Thrift host"),
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
            Arg::with_name(ARG_BOUND_ADDR_FILE)
                .long(ARG_BOUND_ADDR_FILE)
                .required(false)
                .takes_value(true)
                .help("path for file in which to write the bound tcp address in rust std::net::SocketAddr format"),
        );

    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let runtime = matches.runtime();
    let port = value_t!(matches.value_of(ARG_PORT), u16)?;
    let host = matches.value_of(ARG_HOST).unwrap_or("::");
    let bound_addr_path = matches.value_of(ARG_BOUND_ADDR_FILE).map(|v| v.to_string());
    let config_path = matches
        .value_of("mononoke-config-path")
        .expect("must set config path");

    let exec = runtime.clone();

    let config_store = matches.config_store();
    let repo_configs = load_repo_configs(config_path, config_store)?;

    let scuba_builder = matches.scuba_sample_builder();
    let warm_bookmarks_cache_scuba_builder = matches.warm_bookmarks_cache_scuba_sample_builder();

    let repo_factory = RepoFactory::new(matches.environment().clone(), &repo_configs.common);

    let env = MononokeApiEnvironment {
        repo_factory: repo_factory.clone(),
        disabled_hooks: args::parse_disabled_hooks_with_repo_prefix(&matches, &logger)?,
        warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::AllKinds,
        warm_bookmarks_cache_enabled: true,
        warm_bookmarks_cache_scuba_sample_builder: warm_bookmarks_cache_scuba_builder,
        skiplist_enabled: true,
    };

    let mononoke = Arc::new(runtime.block_on(Mononoke::new(&env, repo_configs.clone()))?);
    let megarepo_api = Arc::new(runtime.block_on(MegarepoApi::new(
        matches.environment(),
        repo_configs,
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
    let fb303 = move |proto| {
        make_FacebookService_server(proto, facebook::FacebookServiceImpl, fb303_base.clone())
    };
    let source_control_server = source_control_impl::SourceControlServiceImpl::new(
        fb,
        mononoke.clone(),
        megarepo_api,
        logger.clone(),
        scuba_builder.clone(),
        args::get_scribe(fb, &matches)?,
    );
    let service = {
        move |proto| {
            make_SourceControlService_server(
                proto,
                source_control_server.thrift_server(),
                fb303.clone(),
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
        .with_address(&host, port, false)?
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

    // Start listening.
    service_framework
        .serve_background()
        .expect("failed to start thrift service");

    let bound_addr = format!("{}:{}", &host, service_framework.get_address()?.get_port()?);
    info!(logger, "Listening on {}", bound_addr);

    // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
    if let Some(bound_addr_path) = bound_addr_path {
        let mut writer = File::create(bound_addr_path)?;
        writer.write_all(bound_addr.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    serve_forever(
        runtime,
        monitoring_forever.map(Result::<(), Error>::Ok),
        &logger,
        move || will_exit.store(true, Ordering::Relaxed),
        args::get_shutdown_grace_period(&matches)?,
        async {
            // Note that async blocks are lazy, so this isn't called until first poll
            let _ = task::spawn_blocking(move || {
                // Calling `stop` blocks until the service has completed all requests.
                service_framework.stop();
            })
            .await;
        },
        args::get_shutdown_timeout(&matches)?,
    )?;

    info!(logger, "Exiting...");
    Ok(())
}
