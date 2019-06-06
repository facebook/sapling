// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]


mod monitoring;

use clap::{App, ArgMatches};
use failure_ext::SlogKVError;
use futures::Future;
use lazy_static::lazy_static;
use loadlimiter;
use metaconfig_parser::RepoConfigs;
use slog::{crit, info, o, Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Runtime;

mod errors {
    pub use failure_ext::{Error, Result};
}
use crate::errors::*;

lazy_static! {
    static ref TERMINATE_PROCESS: AtomicBool = AtomicBool::new(false);
}

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

            -d, --debug                                          'print debug level output'
            "#,
        );
    let app = cmdlib::args::add_myrouter_args(app);
    cmdlib::args::add_cachelib_args(app, false /* hide_advanced_args */)
}

fn setup_logger<'a>(matches: &ArgMatches<'a>) -> Logger {
    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    let drain = {
        let drain = {
            // TODO: switch to TermDecorator, which supports color
            let decorator = slog_term::PlainSyncDecorator::new(io::stdout());
            let stderr_drain = GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer);
            // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
            // since logview is sampled anyway.
            let logview_drain = LogViewDrain::new("errorlog_mononoke").ignore_res();
            slog::Duplicate::new(stderr_drain, logview_drain)
        };
        let drain = slog_stats::StatsDrain::new(drain);
        drain.filter_level(level)
    };

    Logger::root(
        drain.fuse(),
        o!(kv_defaults::FacebookKV::new().expect("Failed to initialize logging")),
    )
}

fn get_config<'a>(matches: &ArgMatches<'a>) -> Result<RepoConfigs> {
    // TODO: This needs to cope with blob repos, too
    let cpath = PathBuf::from(matches.value_of("cpath").unwrap());
    RepoConfigs::read_configs(cpath)
}

fn main() {
    let matches = setup_app().get_matches();
    let root_log = setup_logger(&matches);

    panichandler::set_panichandler(panichandler::Fate::Abort);

    cmdlib::args::init_cachelib(&matches);

    fn run_server<'a>(root_log: &Logger, matches: ArgMatches<'a>) -> Result<!> {
        info!(root_log, "Starting up");

        let stats_aggregation = stats::schedule_stats_aggregation()
            .expect("failed to create stats aggregation scheduler");

        let mut runtime = Runtime::new()?;

        let config = get_config(&matches)?;
        let cert = matches.value_of("cert").unwrap().to_string();
        let private_key = matches.value_of("private_key").unwrap().to_string();
        let ca_pem = matches.value_of("ca_pem").unwrap().to_string();
        let ticket_seed = matches
            .value_of("ssl-ticket-seeds")
            .unwrap_or(secure_utils::fb_tls::SEED_PATH)
            .to_string();

        loadlimiter::init(&config.common);

        let ssl = secure_utils::SslConfig {
            cert,
            private_key,
            ca_pem,
        };

        let myrouter_port = cmdlib::args::parse_myrouter_port(&matches);

        let mut acceptor = secure_utils::build_tls_acceptor_builder(ssl.clone())
            .expect("failed to build tls acceptor");
        acceptor = secure_utils::fb_tls::tls_acceptor_builder(
            root_log.clone(),
            ssl.clone(),
            acceptor,
            ticket_seed,
        ).expect("failed to build fb_tls acceptor");

        let (repo_listeners, ready) = repo_listener::create_repo_listeners(
            config.common,
            config.repos.into_iter(),
            myrouter_port,
            root_log,
            matches
                .value_of("listening-host-port")
                .expect("listening path must be specified"),
            acceptor.build(),
            &TERMINATE_PROCESS,
        );

        tracing_fb303::register();

        let sigterm = 15;
        unsafe {
            signal(sigterm, handle_sig_term);
        }

        // Thread with a thrift service is now detached
        monitoring::start_thrift_service(&root_log, &matches, ready);

        runtime.spawn(
            repo_listeners
                .select(stats_aggregation.from_err())
                .map(|((), _)| ())
                .map_err(|(err, _)| panic!("Unexpected error: {:#?}", err)),
        );
        runtime
            .shutdown_on_idle()
            .wait()
            .expect("This runtime should never be idle");

        info!(root_log, "No service to run, shutting down");
        std::process::exit(0);
    }

    match run_server(&root_log, matches) {
        Ok(_) => panic!("unexpected success"),
        Err(e) => {
            crit!(root_log, "Server fatal error"; SlogKVError(e));
            std::process::exit(1);
        }
    }
}

extern "C" {
    fn signal(sig: u32, cb: extern "C" fn(u32)) -> extern "C" fn(u32);
}

extern "C" fn handle_sig_term(_: u32) {
    TERMINATE_PROCESS.store(true, Ordering::Relaxed);
}
