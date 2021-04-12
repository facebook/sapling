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
use mononoke_api::{
    BookmarkUpdateDelay, Mononoke, MononokeEnvironment, WarmBookmarksCacheDerivedData,
};
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
    let app = args::MononokeAppBuilder::new("mononoke server")
        .with_shutdown_timeout_args()
        .with_all_repos()
        .with_disabled_hooks_args()
        .with_scuba_logging_args()
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
        );

    let app = args::add_mcrouter_args(app);
    let app = args::add_scribe_logging_args(app);
    app
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches();
    cmdlib::args::maybe_enable_mcrouter(fb, &matches);

    let (caching, root_log, runtime) = cmdlib::args::init_mononoke(fb, &matches)?;
    let config_store = cmdlib::args::init_config_store(fb, &root_log, &matches)?;
    let observability_context = cmdlib::args::init_observability_context(fb, &matches, &root_log)?;

    let cslb_config = matches.value_of(ARG_CSLB_CONFIG).map(|s| s.to_string());
    info!(root_log, "Starting up");

    let config = args::load_repo_configs(config_store, &matches)?;

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

    let mysql_options = cmdlib::args::parse_mysql_options(&matches);
    let disabled_hooks = cmdlib::args::parse_disabled_hooks_with_repo_prefix(&matches, &root_log)?;
    let scribe = cmdlib::args::get_scribe(fb, &matches)?;
    let host_port = matches
        .value_of(ARG_LISTENING_HOST_PORT)
        .expect("listening path must be specified")
        .to_string();
    let readonly_storage = cmdlib::args::parse_readonly_storage(&matches);
    let blobstore_options = cmdlib::args::parse_blobstore_options(&matches)?;

    let mut scuba = cmdlib::args::get_scuba_sample_builder(fb, &matches, &root_log)?
        .with_observability_context(observability_context.clone());
    scuba.add_common_server_data();

    let will_exit = Arc::new(AtomicBool::new(false));

    let repo_listeners = {
        cloned!(root_log, service, will_exit);
        async move {
            let repo_factory = RepoFactory::new(
                fb,
                root_log.clone(),
                config_store.clone(),
                mysql_options.clone(),
                blobstore_options.clone(),
                readonly_storage,
                caching,
                config.common.censored_scuba_params.clone(),
            );

            let env = MononokeEnvironment {
                fb,
                logger: root_log.clone(),
                repo_factory,
                mysql_options: mysql_options.clone(),
                readonly_storage,
                config_store,
                disabled_hooks,
                warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::HgOnly,
                warm_bookmarks_cache_delay: BookmarkUpdateDelay::Disallow,
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
                config_store,
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
