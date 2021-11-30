/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(never_type)]

use anyhow::{Context, Result};
use clap::Arg;
use cloned::cloned;
use cmdlib::{args, monitoring::ReadyFlagService};
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures_watchdog::WatchdogExt;
use mononoke_api::{Mononoke, MononokeApiEnvironment, WarmBookmarksCacheDerivedData};
use openssl::ssl::AlpnError;
use repo_factory::RepoFactory;
use slog::{error, info};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const ARG_LISTENING_HOST_PORT: &str = "listening-host-port";
const ARG_THRIFT_PORT: &str = "thrift_port";
const ARG_CERT: &str = "cert";
const ARG_PRIVATE_KEY: &str = "private-key";
const ARG_CA_PEM: &str = "ca-pem";
const ARG_TICKET_SEEDS: &str = "ssl-ticket-seeds";
const ARG_CSLB_CONFIG: &str = "cslb-config";

fn setup_app<'a, 'b>() -> args::MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("mononoke server")
        .with_shutdown_timeout_args()
        .with_all_repos()
        .with_disabled_hooks_args()
        .with_scuba_logging_args()
        .with_mcrouter_args()
        .with_scribe_args()
        .with_default_scuba_dataset("mononoke_test_perf")
        .build()
        .about("serve repos")
        .arg(
            Arg::with_name(ARG_LISTENING_HOST_PORT)
                .long(ARG_LISTENING_HOST_PORT)
                .required(true)
                .takes_value(true)
                .help("tcp address to listen to in format `host:port`"),
        )
        .arg(
            Arg::with_name(ARG_THRIFT_PORT)
                .long(ARG_THRIFT_PORT)
                .short("-p")
                .required(false)
                .takes_value(true)
                .help("if provided the thrift server will start on this port"),
        )
        .arg(
            Arg::with_name(ARG_CERT)
                .long(ARG_CERT)
                .required(true)
                .takes_value(true)
                .help("path to a file with certificate"),
        )
        .arg(
            Arg::with_name(ARG_PRIVATE_KEY)
                .long(ARG_PRIVATE_KEY)
                .required(true)
                .takes_value(true)
                .help("path to a file with private key"),
        )
        .arg(
            Arg::with_name(ARG_CA_PEM)
                .long(ARG_CA_PEM)
                .takes_value(true)
                .help("path to a file with CA certificate"),
        )
        .arg(
            Arg::with_name(ARG_TICKET_SEEDS)
                .long(ARG_TICKET_SEEDS)
                .takes_value(true)
                .help("path to a file with encryption keys for SSL tickets'"),
        )
        .arg(
            Arg::with_name(ARG_CSLB_CONFIG)
                .long(ARG_CSLB_CONFIG)
                .takes_value(true)
                .required(false)
                .help("top level Mononoke tier where CSLB publishes routing table"),
        )
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;

    let root_log = matches.logger();
    let runtime = matches.runtime();
    let config_store = matches.config_store().clone();

    let cslb_config = matches.value_of(ARG_CSLB_CONFIG).map(|s| s.to_string());
    info!(root_log, "Starting up");

    let config = args::load_repo_configs(&config_store, &matches)?;

    let acceptor = {
        let cert = matches.value_of(ARG_CERT).unwrap().to_string();
        let private_key = matches.value_of(ARG_PRIVATE_KEY).unwrap().to_string();
        let ca_pem = matches.value_of(ARG_CA_PEM).unwrap().to_string();

        let mut builder = secure_utils::SslConfig::new(
            ca_pem,
            cert,
            private_key,
            matches.value_of(ARG_TICKET_SEEDS),
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

    let disabled_hooks = cmdlib::args::parse_disabled_hooks_with_repo_prefix(&matches, &root_log)?;
    let scribe = cmdlib::args::get_scribe(fb, &matches)?;
    let host_port = matches
        .value_of(ARG_LISTENING_HOST_PORT)
        .expect("listening path must be specified")
        .to_string();

    let mysql_options = matches.mysql_options().clone();
    let readonly_storage = matches.readonly_storage().clone();
    let blobstore_options = matches.blobstore_options().clone();
    let env = matches.environment();

    let scuba = matches.scuba_sample_builder();
    let warm_bookmarks_cache_scuba = matches.warm_bookmarks_cache_scuba_sample_builder();

    let will_exit = Arc::new(AtomicBool::new(false));

    let repo_listeners = {
        cloned!(root_log, service, will_exit, env);
        async move {
            let repo_factory = RepoFactory::new(env, &config.common);

            let env = MononokeApiEnvironment {
                repo_factory,
                disabled_hooks,
                warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::HgOnly,
                warm_bookmarks_cache_enabled: true,
                warm_bookmarks_cache_scuba_sample_builder: warm_bookmarks_cache_scuba,
                skiplist_enabled: true,
            };

            let mononoke = Mononoke::new(&env, config.clone())
                .watched(&root_log)
                .await?;
            info!(&root_log, "Built Mononoke");

            repo_listener::create_repo_listeners(
                fb,
                config.common,
                mononoke,
                &blobstore_options,
                &mysql_options,
                root_log,
                host_port,
                acceptor,
                service,
                terminate_receiver,
                &config_store,
                readonly_storage,
                scribe,
                &scuba,
                will_exit,
                cslb_config,
            )
            .await
        }
    };

    // Thread with a thrift service is now detached
    monitoring::start_thrift_service(fb, &root_log, &matches, service);

    cmdlib::helpers::serve_forever(
        runtime,
        repo_listeners,
        &root_log,
        move || will_exit.store(true, Ordering::Relaxed),
        args::get_shutdown_grace_period(&matches)?,
        async {
            match terminate_sender.send(()) {
                Err(err) => error!(root_log, "could not send termination signal: {:?}", err),
                _ => {}
            }
            repo_listener::wait_for_connections_closed(&root_log).await;
        },
        args::get_shutdown_timeout(&matches)?,
    )
}
