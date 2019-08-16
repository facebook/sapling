// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::{err_msg, format_err, Result};
use filestore::{self, Alias, FetchKey, StoreRequest};
use futures::{Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{
    hash::{Sha1, Sha256},
    ContentId,
};
use slog::{info, Logger};
use std::convert::TryInto;
use std::io::BufReader;
use std::str::FromStr;
use tokio::{codec, fs::File};

use crate::error::SubcommandError;

const COMMAND_METADATA: &str = "metadata";
const COMMAND_STORE: &str = "store";
const COMMAND_VERIFY: &str = "verify";

const ARG_KIND: &str = "kind";
const ARG_ID: &str = "id";
const ARG_FILE: &str = "file";

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
        .about("inspect and interact with filestore data")
        .subcommand(
            SubCommand::with_name(COMMAND_METADATA)
                .arg(kind_arg.clone())
                .arg(id_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_STORE).arg(
                Arg::with_name(ARG_FILE)
                    .help("File")
                    .takes_value(true)
                    .required(true),
            ),
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
) -> BoxFuture<(), SubcommandError> {
    args::init_cachelib(&matches);
    let blobrepo = args::open_repo(&logger, &matches);
    let ctx = CoreContext::new_with_logger(logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_METADATA, Some(matches)) => (blobrepo, extract_fetch_key(matches).into_future())
            .into_future()
            .and_then({
                move |(blobrepo, key)| {
                    filestore::get_metadata(&blobrepo.get_blobstore(), ctx, &key)
                        .inspect({
                            cloned!(logger);
                            move |r| info!(logger, "{:?}", r)
                        })
                        .map(|_| ())
                }
            })
            .from_err()
            .boxify(),
        (COMMAND_STORE, Some(matches)) => (
            blobrepo,
            File::open(matches.value_of(ARG_FILE).unwrap().to_string())
                .and_then(|file| file.metadata())
                .from_err(),
        )
            .into_future()
            .and_then(|(blobrepo, (file, metadata))| {
                let file_buf = BufReader::new(file);
                // If the size doesn't fit into a u64, we aren't going to be able to process
                // it anyway.
                let len: u64 = metadata.len().try_into().unwrap();

                let data = codec::FramedRead::new(file_buf, codec::BytesCodec::new())
                    .map(|bytes_mut| bytes_mut.freeze())
                    .from_err();

                let req = StoreRequest::new(len);

                blobrepo.upload_file(ctx, &req, data)
            })
            .map({
                cloned!(logger);
                move |metadata| {
                    info!(
                        logger,
                        "Wrote {} ({} bytes)", metadata.content_id, metadata.total_size
                    );
                    ()
                }
            })
            .from_err()
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
                            use Alias::*;

                            (
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &FetchKey::Canonical(metadata.content_id),
                                )
                                .then(Ok),
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &FetchKey::Aliased(Sha1(metadata.sha1)),
                                )
                                .then(Ok),
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &FetchKey::Aliased(Sha256(metadata.sha256)),
                                )
                                .then(Ok),
                                filestore::fetch(
                                    &blobstore,
                                    ctx.clone(),
                                    &FetchKey::Aliased(GitSha1(metadata.git_sha1)),
                                )
                                .then(Ok),
                            )
                        }
                    })
                    .map({
                        cloned!(logger);
                        move |(content_id, sha1, sha256, git_sha1)| {
                            info!(logger, "content_id: {:?}", content_id.is_ok());
                            info!(logger, "sha1: {:?}", sha1.is_ok());
                            info!(logger, "sha256: {:?}", sha256.is_ok());
                            info!(logger, "git_sha1: {:?}", git_sha1.is_ok());
                        }
                    })
            })
            .from_err()
            .boxify(),
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}

// NOTE: This assumes the matches are from a command that has ARG_KIND and ARG_ID.
fn extract_fetch_key(matches: &ArgMatches<'_>) -> Result<FetchKey> {
    let id = matches.value_of(ARG_ID).unwrap();

    match matches.value_of(ARG_KIND).unwrap() {
        "id" => Ok(FetchKey::Canonical(ContentId::from_str(id)?)),
        "sha1" => Ok(FetchKey::Aliased(Alias::Sha1(Sha1::from_str(id)?))),
        "sha256" => Ok(FetchKey::Aliased(Alias::Sha256(Sha256::from_str(id)?))),
        kind => Err(format_err!("Invalid kind: {}", kind)),
    }
}
