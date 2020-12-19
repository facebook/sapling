/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, format_err, Error, Result};
use blobstore::Loadable;
use bytes::BytesMut;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::{self, Alias, FetchKey, StoreRequest};
use futures::{
    future::{self, TryFutureExt},
    stream::TryStreamExt,
};
use mononoke_types::{
    hash::{Sha1, Sha256},
    ContentId, FileContents,
};
use slog::{info, Logger};
use std::str::FromStr;
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufReader},
};
use tokio_util::codec::{BytesCodec, FramedRead};

use crate::error::SubcommandError;

pub const FILESTORE: &str = "filestore";
const COMMAND_METADATA: &str = "metadata";
const COMMAND_STORE: &str = "store";
const COMMAND_FETCH: &str = "fetch";
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
            SubCommand::with_name(COMMAND_FETCH)
                .arg(kind_arg.clone())
                .arg(id_arg.clone()),
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
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    args::init_cachelib(fb, &matches);
    let blobrepo = args::open_repo(fb, &logger, &matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_METADATA, Some(matches)) => {
            let key = extract_fetch_key(matches)?;
            let result = filestore::get_metadata(blobrepo.blobstore(), &ctx, &key).await;
            info!(logger, "{:?}", result);
            let _ = result?;
            Ok(())
        }
        (COMMAND_STORE, Some(matches)) => {
            let file = matches.value_of(ARG_FILE).unwrap().to_string();
            let file = File::open(&file).await.map_err(Error::from)?;
            let metadata = file.metadata().await.map_err(Error::from)?;

            let data = BufReader::new(file);
            let data = FramedRead::new(data, BytesCodec::new()).map_ok(BytesMut::freeze);
            let len = metadata.len();
            let metadata = filestore::store(
                blobrepo.blobstore(),
                blobrepo.filestore_config(),
                &ctx,
                &StoreRequest::new(len),
                data.map_err(Error::from),
            )
            .await?;
            info!(
                logger,
                "Wrote {} ({} bytes)", metadata.content_id, metadata.total_size
            );
            Ok(())
        }
        (COMMAND_FETCH, Some(matches)) => {
            let fetch_key = extract_fetch_key(matches)?;
            let mut stream = filestore::fetch(blobrepo.blobstore(), ctx.clone(), &fetch_key)
                .await?
                .ok_or_else(|| anyhow!("content not found"))?;

            let mut stdout = tokio::io::stdout();

            while let Some(b) = stream.try_next().await? {
                stdout.write_all(b.as_ref()).await.map_err(Error::from)?;
            }

            Ok(())
        }
        (COMMAND_VERIFY, Some(matches)) => {
            let key = extract_fetch_key(matches)?;
            let blobstore = blobrepo.get_blobstore();

            let metadata = filestore::get_metadata(&blobstore, &ctx, &key).await?;
            let metadata = match metadata {
                Some(metadata) => metadata,
                None => return Err(Error::msg("Content not found!").into()),
            };

            use filestore::Alias::*;

            let (content_id, sha1, sha256, git_sha1) = future::join4(
                filestore::fetch(
                    &blobstore,
                    ctx.clone(),
                    &FetchKey::Canonical(metadata.content_id),
                ),
                filestore::fetch(
                    &blobstore,
                    ctx.clone(),
                    &FetchKey::Aliased(Sha1(metadata.sha1)),
                ),
                filestore::fetch(
                    &blobstore,
                    ctx.clone(),
                    &FetchKey::Aliased(Sha256(metadata.sha256)),
                ),
                filestore::fetch(
                    &blobstore,
                    ctx.clone(),
                    &FetchKey::Aliased(GitSha1(metadata.git_sha1.sha1())),
                ),
            )
            .await;

            info!(logger, "content_id: {:?}", content_id.is_ok());
            info!(logger, "sha1: {:?}", sha1.is_ok());
            info!(logger, "sha256: {:?}", sha256.is_ok());
            info!(logger, "git_sha1: {:?}", git_sha1.is_ok());

            Ok(())
        }
        (COMMAND_IS_CHUNKED, Some(matches)) => {
            let fetch_key = extract_fetch_key(matches)?;
            let maybe_metadata =
                filestore::get_metadata(&blobrepo.get_blobstore(), &ctx, &fetch_key).await?;
            match maybe_metadata {
                Some(metadata) => {
                    let file_contents = metadata
                        .content_id
                        .load(&ctx, &blobrepo.get_blobstore())
                        .map_err(Error::from)
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
        _ => Err(SubcommandError::InvalidArgs),
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
