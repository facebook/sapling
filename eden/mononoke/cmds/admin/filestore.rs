/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Error, Result};
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::{self, Alias, FetchKey, StoreRequest};
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use futures_ext::FutureExt as OldFutureExt;
use futures_old::{Future, IntoFuture, Stream};
use mononoke_types::{
    hash::{Sha1, Sha256},
    ContentId, FileContents,
};
use slog::{info, Logger};
use std::convert::TryInto;
use std::io::BufReader;
use std::str::FromStr;
use tokio_old::{codec, fs::File};

use crate::error::SubcommandError;

pub const FILESTORE: &str = "filestore";
const COMMAND_METADATA: &str = "metadata";
const COMMAND_STORE: &str = "store";
const COMMAND_VERIFY: &str = "verify";
const COMMAND_IS_CHUNKED: &str = "is-chunked";

const ARG_KIND: &str = "kind";
const ARG_ID: &str = "id";
const ARG_FILE: &str = "file";

// NOTE: Fetching by GitSha1 is not concurrently supported since that needs a size to instantiate.
const VALID_KINDS: [&str; 3] = ["id", "sha1", "sha256"];

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let kind_arg = Arg::with_name(ARG_KIND)
        .possible_values(&VALID_KINDS)
        .help("Identifier kind")
        .takes_value(true)
        .required(true);

    let id_arg = Arg::with_name(ARG_ID)
        .help("Identifier")
        .takes_value(true)
        .required(true);

    SubCommand::with_name(FILESTORE)
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
        .subcommand(
            SubCommand::with_name(COMMAND_IS_CHUNKED)
                .arg(kind_arg.clone())
                .arg(id_arg.clone()),
        )
}

pub async fn execute_command<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    args::init_cachelib(fb, &matches, None);
    let blobrepo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_METADATA, Some(matches)) => (blobrepo, extract_fetch_key(matches).into_future())
            .into_future()
            .and_then({
                move |(blobrepo, key)| {
                    filestore::get_metadata(blobrepo.blobstore(), ctx, &key)
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
                    .map(bytes_ext::copy_from_old)
                    .from_err();
                filestore::store(
                    blobrepo.get_blobstore(),
                    blobrepo.filestore_config(),
                    ctx,
                    &StoreRequest::new(len),
                    data,
                )
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
                        None => bail!("Content not found!"),
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
                                    &FetchKey::Aliased(GitSha1(metadata.git_sha1.sha1())),
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
        (COMMAND_IS_CHUNKED, Some(matches)) => {
            let fetch_key = extract_fetch_key(matches)?;
            async move {
                let repo = blobrepo.compat().await?;
                let maybe_metadata =
                    filestore::get_metadata(&repo.get_blobstore(), ctx.clone(), &fetch_key)
                        .compat()
                        .await?;
                match maybe_metadata {
                    Some(metadata) => {
                        let file_contents = metadata
                            .content_id
                            .load(ctx, &repo.get_blobstore())
                            .map_err(Error::from)
                            .compat()
                            .await?;
                        match file_contents {
                            FileContents::Bytes(_) => {
                                println!("not chunked");
                            }
                            FileContents::Chunked(_) => {
                                println!("chunked");
                            }
                        }
                    }
                    None => {
                        println!("contentid not found");
                    }
                }
                Ok(())
            }
            .boxed()
            .compat()
            .boxify()
        }
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
    .compat()
    .await
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
