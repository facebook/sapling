/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

mod monitoring;

use anyhow::Result;
use clap::{App, ArgMatches};
use cmdlib::args;
use configerator_cached::ConfigStore;
use fbinit::FacebookInit;
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt, TryFutureExt},
};
use metaconfig_parser::RepoConfigs;
use slog::{error, info};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = App::new("mononoke server")
        .version("0.0.0")
        .about("serve repos")
        .args_from_usage(
            r#"
            <cpath>      -P, --config_path [PATH]           'path to the config files'

                          --listening-host-port <PATH>           'tcp address to listen to in format `host:port`'

            -p, --thrift_port [PORT] 'if provided the thrift server will start on this port'

            <cert>        --cert [PATH]                         'path to a file with certificate'
            <private_key> --private-key [PATH]                  'path to a file with private key'
            <ca_pem>      --ca-pem [PATH]                       'path to a file with CA certificate'
            [ticket_seed] --ssl-ticket-seeds [PATH]             'path to a file with encryption keys for SSL tickets'

            --test-instance                                     'disables some functionality for tests'
            --local-configerator-path [PATH]                    'local path to fetch configerator configs from. used only if --test-instance is '
            "#,
        );
    let app = cmdlib::args::add_shutdown_timeout_args(app);
    let app = cmdlib::args::add_mysql_options_args(app);
    let app = cmdlib::args::add_mcrouter_args(app);
    let app = cmdlib::args::add_cachelib_args(app, false /* hide_advanced_args */);
    let app = cmdlib::args::add_disabled_hooks_args(app);
    let app = cmdlib::args::add_logger_args(app);
    let app = cmdlib::args::add_blobstore_args(app);
    app
}

fn get_config<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    let cpath = PathBuf::from(matches.value_of("cpath").unwrap());
    RepoConfigs::read_configs(fb, cpath)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches();
    cmdlib::args::maybe_enable_mcrouter(fb, &matches);
    let root_log = cmdlib::args::init_logging(fb, &matches);

    info!(root_log, "Starting up");

    let config = get_config(fb, &matches)?;
    let cert = matches.value_of("cert").unwrap().to_string();
    let private_key = matches.value_of("private_key").unwrap().to_string();
    let ca_pem = matches.value_of("ca_pem").unwrap().to_string();
    let ticket_seed = matches
        .value_of("ssl-ticket-seeds")
        .unwrap_or(secure_utils::fb_tls::SEED_PATH)
        .to_string();

    let ssl = secure_utils::SslConfig {
        cert,
        private_key,
        ca_pem,
    };

    let mut acceptor = secure_utils::build_tls_acceptor_builder(ssl.clone())
        .expect("failed to build tls acceptor");
    acceptor = secure_utils::fb_tls::tls_acceptor_builder(
        root_log.clone(),
        ssl.clone(),
        acceptor,
        ticket_seed,
    )
    .expect("failed to build fb_tls acceptor");

    let test_instance = matches.is_present("test-instance");
    let config_source = if test_instance {
        let local_configerator_path = matches.value_of("local-configerator-path");
        local_configerator_path.map(|path| {
            ConfigStore::file(
                root_log.clone(),
                PathBuf::from(path),
                String::new(),
                CONFIGERATOR_POLL_INTERVAL,
            )
        })
    } else {
        Some(
            ConfigStore::configerator(
                fb,
                root_log.clone(),
                CONFIGERATOR_POLL_INTERVAL,
                CONFIGERATOR_REFRESH_TIMEOUT,
            )
            .expect("can't set up configerator API"),
        )
    };

    let runtime = cmdlib::args::init_runtime(&matches)?;

    info!(root_log, "Creating repo listeners");

    let terminate = Arc::new(AtomicBool::new(false));

    let (repo_listeners, ready) = repo_listener::create_repo_listeners(
        fb,
        config.common,
        config.repos.into_iter(),
        cmdlib::args::parse_mysql_options(&matches),
        cmdlib::args::init_cachelib(fb, &matches),
        cmdlib::args::parse_disabled_hooks_with_repo_prefix(&matches, &root_log)?,
        &root_log,
        matches
            .value_of("listening-host-port")
            .expect("listening path must be specified"),
        acceptor.build(),
        terminate.clone(),
        config_source,
        cmdlib::args::parse_readonly_storage(&matches),
        cmdlib::args::parse_blobstore_options(&matches),
    );

    tracing_fb303::register(fb);

    // Thread with a thrift service is now detached
    monitoring::start_thrift_service(fb, &root_log, &matches, ready);

    cmdlib::helpers::serve_forever(
        runtime,
        repo_listeners
            .compat()
            .map_err({
                let logger = root_log.clone();
                move |e| error!(&logger, "Unhandled error: {:?}", e)
            })
            .map(|_| ()),
        &root_log,
        || {},
        args::get_shutdown_grace_period(&matches)?,
        async {
            terminate.store(true, Ordering::Relaxed);
            repo_listener::wait_for_connections_closed().await;
        },
        args::get_shutdown_timeout(&matches)?,
    )
}
