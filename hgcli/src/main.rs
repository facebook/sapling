// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate clap;
extern crate tokio_uds;
#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate bytes;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;

extern crate mio;
extern crate nix;

extern crate futures_ext;
extern crate sshrelay;

use clap::{App, Arg, SubCommand};

mod serve;
mod errors;

fn main() {
    let matches = App::new("Mononoke CLI")
        .about("Provide minimally compatible CLI to Mononoke server")
        .arg(Arg::from_usage("-R, --repository=<REPO> 'repository name'"))
        .subcommand(
            SubCommand::with_name("serve")
                .about("start server")
                .arg(Arg::from_usage(
                    "-A, --accesslog [FILE] 'name of access log file'",
                ))
                .arg(Arg::from_usage("-d, --daemon 'run server in background'"))
                .arg(Arg::from_usage(
                    "-E, --errorlog [FILE] 'name of error log file to write to'",
                ))
                .arg(
                    Arg::from_usage("-p, --port <PORT> 'port to listen on'").default_value("8000"),
                )
                .arg(Arg::from_usage(
                    "-a, --address [ADDR] 'address to listen on'",
                ))
                .arg(Arg::from_usage("--stdio 'for remote clients'"))
                .arg(
                    Arg::from_usage("--cmdserver [MODE] 'for remote clients'")
                        .possible_values(&["pipe", "unix"]),
                ),
        )
        .get_matches();

    let res = if let Some(subcmd) = matches.subcommand_matches("serve") {
        serve::cmd(&matches, subcmd)
    } else {
        Err("unexpected or missing subcommand".into())
    };

    if let Err(err) = res {
        println!("Subcommand failed: {:?}", err);
    }
}
