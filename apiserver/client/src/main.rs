// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate apiserver_client as client;
extern crate clap;
extern crate futures;
extern crate futures_ext;
extern crate tokio;

use std::string::String;

use clap::{App, Arg, ArgMatches, SubCommand};
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use client::MononokeAPIClient;

fn cat<'a>(client: MononokeAPIClient, matches: &ArgMatches<'a>) -> BoxFuture<(), ()> {
    let changeset = matches
        .value_of("changeset")
        .expect("must provide changeset");
    let path = matches.value_of("path").expect("must provide path");

    client
        .get_raw(changeset.to_string(), path.to_string())
        .map_err(|e| eprintln!("error: {}", e))
        .and_then(|r| String::from_utf8(r).map_err(|_| eprintln!("error: utf8 conversion failed.")))
        .map(|res| {
            println!("{}", res);
        })
        .boxify()
}

fn main() -> Result<(), ()> {
    let matches = App::new("Mononoke API Server Thrift client")
        .about("Send requests to Mononoke API Server thrift port")
        .arg(
            Arg::with_name("tier")
                .short("t")
                .long("tier")
                .value_name("TIER")
                .help("tier name")
                .default_value("mononoke-apiserver-thrift"),
        )
        .arg(
            Arg::with_name("repo")
                .short("r")
                .long("repo")
                .value_name("NAME")
                .help("repository name (e.g. fbsource)")
                .default_value("fbsource"),
        )
        .subcommand(
            SubCommand::with_name("cat")
                .about("retrieve file content")
                .arg(
                    Arg::with_name("changeset")
                        .short("c")
                        .long("changeset")
                        .value_name("HASH")
                        .help("hash of the changeset you want to query")
                        .required(true),
                )
                .arg(
                    Arg::with_name("path")
                        .short("p")
                        .long("path")
                        .value_name("PATH")
                        .help("path to the file you want to get")
                        .required(true),
                ),
        )
        .get_matches();

    let tier = matches.value_of("tier").expect("must provide tier name");
    let repo = matches.value_of("repo").expect("must provide repo name");

    let client =
        MononokeAPIClient::new_with_tier_repo(tier, repo).map_err(|e| eprintln!("error: {}", e))?;

    let future = if let Some(matches) = matches.subcommand_matches("cat") {
        cat(client, matches)
    } else {
        Ok(()).into_future().boxify()
    };

    tokio::run(future);

    Ok(())
}
