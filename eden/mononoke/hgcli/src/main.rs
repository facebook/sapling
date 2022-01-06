/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use clap::{value_t, App, Arg, ArgGroup, SubCommand};
use rand::Rng;
use std::time::Duration;

mod serve;

const ARG_RUNTIME_THREADS: &str = "runtime-threads";
const ARG_FORCE_FB: &str = "force-fb";
const ARG_NO_FB: &str = "no-fb";

pub fn main() {
    let matches = App::new("Mononoke CLI")
        .about("Provide minimally compatible CLI to Mononoke server")
        .arg(Arg::from_usage("-R, --repository=<REPO> 'repository name'"))
        .arg(Arg::from_usage(
            "--query-string [QUERY_STRING] 'original query string passed to repository path'",
        ))
        .arg(Arg::from_usage("--remote-proxy 'hgcli is run as remote proxy, not locally'"))
        .arg(Arg::from_usage(
            "--scuba-table [SCUBA_TABLE] 'name of scuba table to log to'",
        ))
        .arg(Arg::from_usage(
            "--no-session-output 'disables the session uuid output'",
        ))
        .arg(
            Arg::with_name("priority")
                .long("priority")
                .takes_value(true)
                .required(false)
                .help("Set request priority"),
        )
        .arg(
            Arg::with_name(ARG_RUNTIME_THREADS)
                .long(ARG_RUNTIME_THREADS)
                .takes_value(true)
                .default_value("1")
                .help("a number of threads to use in the tokio runtime (default: 1)"),
        )
        .arg(
            Arg::with_name(ARG_FORCE_FB)
                .long(ARG_FORCE_FB)
                .takes_value(false)
                .required(false)
        )
        .arg(
            Arg::with_name(ARG_NO_FB)
                .long(ARG_NO_FB)
                .takes_value(false)
                .required(false)
        )
        .group(ArgGroup::with_name("fb").args(&[ARG_FORCE_FB, ARG_NO_FB]))
        .subcommand(
            SubCommand::with_name("serve")
                .about("start server")
                .arg(Arg::from_usage(
                    "--mononoke-path <PATH> 'path to connect to mononoke server'",
                ))
                .arg(Arg::from_usage(
                    "-A, --accesslog [FILE] 'name of access log file'",
                ))
                .arg(Arg::from_usage("-d, --daemon 'run server in background'"))
                .arg(Arg::from_usage(
                    "-E, --errorlog [FILE] 'name of error log file to write to'",
                ))
                .arg(Arg::from_usage("-p, --port <PORT> 'port to listen on'").default_value("8000"))
                .arg(Arg::from_usage(
                    "-a, --address [ADDR] 'address to listen on'",
                ))
                .arg(Arg::from_usage(
                    "--cert [CERT]  'path to the certificate file'",
                ))
                .arg(Arg::from_usage("--ca-pem [PEM] 'path to the pem file'"))
                .arg(Arg::from_usage(
                    "--private-key [KEY] 'path to the private key'",
                ))
                .arg(Arg::from_usage("--insecure 'run hgcli without verifying peer certificate'"))
                .arg(Arg::from_usage("--stdio 'for remote clients'"))
                .arg(
                    Arg::from_usage("--cmdserver [MODE] 'for remote clients'")
                        .possible_values(&["pipe", "unix"]),
                )
                .arg(Arg::from_usage(
                    "--mock-username [USERNAME] 'use only in tests, send this username instead of the currently logged in'",
                ))
                .arg(Arg::from_usage(
                    "--client-debug 'tell mononoke to send debug information to the client'",
                ))
                .arg(
                    Arg::with_name(serve::ARG_COMMON_NAME)
                        .long(serve::ARG_COMMON_NAME)
                        .takes_value(true)
                        .required(false)
                        .help("expected SSL common name of the server see https://www.ssl.com/faqs/common-name/"),
                )
                .arg(
                    Arg::with_name(serve::ARG_SERVER_CERT_IDENTITY)
                        .long(serve::ARG_SERVER_CERT_IDENTITY)
                        .takes_value(true)
                        .required(false)
                        .help("expected identity of the server"),
                )
                .group(
                    ArgGroup::with_name("idents")
                        .args(&[serve::ARG_COMMON_NAME, serve::ARG_SERVER_CERT_IDENTITY])
                        .required(true)
                ),
        )
        .get_matches();

    let core_threads = match value_t!(matches.value_of(ARG_RUNTIME_THREADS), usize) {
        Ok(core_threads) => core_threads,
        Err(err) => {
            eprintln!("{:?}", err);
            return;
        }
    };

    let use_fb = if matches.is_present(ARG_NO_FB) {
        false
    } else if matches.is_present(ARG_FORCE_FB) {
        true
    } else {
        rand::thread_rng().gen_range(0..100) == 0usize
    };

    let (fb, mut fb_destroy_guard) = if use_fb {
        unsafe {
            let fb = fbinit::r#impl::perform_init();
            let fb_destroy_guard = fbinit::r#impl::DestroyGuard::new();
            (Some(fb), Some(fb_destroy_guard))
        }
    } else {
        (None, None)
    };


    let res = if let Some(subcmd) = matches.subcommand_matches("serve") {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(core_threads)
            .enable_all()
            .build()
            .map_err(Error::from)
            .and_then(|runtime| {
                let result = runtime.block_on(serve::cmd(fb, &matches, subcmd));

                // NOTE: We leak the runtime, and all its tasks here. This is very unfortunate, but
                // it's due to the fact that we use Stdin / Stdout and Stderr from Tokio, and those
                // things spawn blocking threads, which will prevent the runtime from shutting down
                // if they are busy (P163996251 for what this looks like).
                //
                // This would normally be OK for most apps, but hgcli is a bit special in the sense
                // that its client does not close stdin, so stdin will be blocked waiting for input
                // when we get here.
                //
                // However, at this point, there is no use waiting on further input, since by the
                // point we return from serve::cmd, we have successfully flushed stdout & stderr,
                // and expect no further input on stdin (and even if we had any, we wouldn't do
                // anything with it).
                //
                // We have two ways of doing this. We could std::mem::forget about the runtime, but
                // then if we have anything on the runtime that *isn't* a read from stdin, we could
                // block the fbinit destructors from running. So, what we do is we drop the runtime
                // on a separate thread. If it blocks fbinit then we'll implicitly wait until it
                // shuts down "enough", and if it doesn't we'll just let the runtime leak, which
                // doesn't matter, because at this point we are done and we just REALLY WOULD LIKE
                // THIS PROGRAM TO JUST EXIT.

                let _ = std::thread::spawn(|| runtime.shutdown_timeout(Duration::from_nanos(0)));

                result
            })
    } else {
        Err(Error::msg("unexpected or missing subcommand"))
    };

    if let Err(err) = res {
        eprintln!("Subcommand failed: {:?}", err);
    }

    fb_destroy_guard.take();
}
