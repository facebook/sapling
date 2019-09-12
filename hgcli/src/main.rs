// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
// TODO: (rain1) T21726029 tokio/futures deprecated a bunch of stuff, clean it all up
#![allow(deprecated)]

use clap::{App, Arg, SubCommand};
use failure_ext as failure;

mod serve;

pub mod errors {
    pub use failure_ext::{Error, Result};
}
use crate::errors::Error;

fn main() {
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
                .arg(Arg::from_usage(
                    "--common-name [CN] 'expected SSL common name of the server see https://www.ssl.com/faqs/common-name/'",
                ))
                .arg(Arg::from_usage("--insecure 'run hgcli without verifying peer certificate'"))
                .arg(Arg::from_usage("--stdio 'for remote clients'"))
                .arg(
                    Arg::from_usage("--cmdserver [MODE] 'for remote clients'")
                        .possible_values(&["pipe", "unix"]),
                )
                .arg(Arg::from_usage(
                    "--mock-username [USERNAME] 'use only in tests, send this username instead of the currently logged in'",
                )),
        )
        .get_matches();

    let res = if let Some(subcmd) = matches.subcommand_matches("serve") {
        tokio::runtime::Runtime::new()
            .map_err(Error::from)
            .and_then(|mut runtime| {
                let result = runtime.block_on(serve::cmd(&matches, subcmd));
                runtime.shutdown_on_idle();
                result
            })
    } else {
        Err(failure::err_msg("unexpected or missing subcommand"))
    };

    if let Err(err) = res {
        eprintln!("Subcommand failed: {:?}", err);
    }
}
