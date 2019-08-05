// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error, Result};
use filestore::{self, FetchKey};
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{
    hash::{Sha1, Sha256},
    ContentId,
};
use slog::Logger;
use std::str::FromStr;

const COMMAND_METADATA: &str = "metadata";
const COMMAND_VERIFY: &str = "verify";

const ARG_KIND: &str = "kind";
const ARG_ID: &str = "id";

// NOTE: Fetching by GitSha1 is not concurrently supported since that needs a size to instantiate.
const VALID_KINDS: [&str; 3] = ["id", "sha1", "sha256"];

pub fn build_subcommand(name: &str) -> App {
    let kind_arg = Arg::with_name(ARG_KIND)
        .possible_values(&VALID_KINDS)
        .help("Identifier kind")
        .takes_value(true)
        .required(true);

    let id_arg = Arg::with_name(ARG_ID)
        .help("Identifier")
        .takes_value(true)
        .required(true);

    SubCommand::with_name(name)
        .about("inspect filestore data")
        .subcommand(
            SubCommand::with_name(COMMAND_METADATA)
                .arg(kind_arg.clone())
                .arg(id_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VERIFY)
                .arg(kind_arg.clone())
                .arg(id_arg.clone()),
        )
}

pub fn execute_command(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_matches: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let blobrepo = args::open_repo(&logger, &matches);
    let ctx = CoreContext::test_mock();

    match sub_matches.subcommand() {
        (COMMAND_METADATA, Some(matches)) => (blobrepo, extract_fetch_key(matches).into_future())
            .into_future()
            .and_then(move |(blobrepo, key)| {
                filestore::get_metadata(&blobrepo.get_blobstore(), ctx, &key)
                    .inspect(|r| println!("{:?}", r))
                    .map(|_| ())
            })
            .boxify(),
        (COMMAND_VERIFY, Some(matches)) => (blobrepo, extract_fetch_key(matches).into_future())
            .into_future()
            .and_then(move |(blobrepo, key)| {
                let blobstore = blobrepo.get_blobstore();

                filestore::get_metadata(&blobstore, ctx.clone(), &key)
                    .and_then(|metadata| match metadata {
                        Some(metadata) => Ok(metadata),
                        None => Err(err_msg("Content not found!")),
                    })
                    .and_then({
                        cloned!(blobstore, ctx);
                        move |metadata| {
                            use FetchKey::*;

                            (
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &Canonical(metadata.content_id),
                                )
                                .then(Ok),
                                filestore::fetch(&blobstore, ctx.clone(), &Sha1(metadata.sha1))
                                    .then(Ok),
                                filestore::fetch(&blobstore, ctx.clone(), &Sha256(metadata.sha256))
                                    .then(Ok),
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &GitSha1(metadata.git_sha1),
                                )
                                .then(Ok),
                            )
                        }
                    })
                    .map(|(content_id, sha1, sha256, git_sha1)| {
                        println!("content_id: {:?}", content_id.is_ok());
                        println!("sha1: {:?}", sha1.is_ok());
                        println!("sha256: {:?}", sha256.is_ok());
                        println!("git_sha1: {:?}", git_sha1.is_ok());
                    })
            })
            .boxify(),
        _ => {
            eprintln!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

// NOTE: This assumes the matches are from a command that has ARG_KIND and ARG_ID.
fn extract_fetch_key(matches: &ArgMatches<'_>) -> Result<FetchKey> {
    let id = matches.value_of(ARG_ID).unwrap();

    match matches.value_of(ARG_KIND).unwrap() {
        "id" => Ok(FetchKey::Canonical(ContentId::from_str(id)?)),
        "sha1" => Ok(FetchKey::Sha1(Sha1::from_str(id)?)),
        "sha256" => Ok(FetchKey::Sha256(Sha256::from_str(id)?)),
        kind => Err(format_err!("Invalid kind: {}", kind)),
    }
}
