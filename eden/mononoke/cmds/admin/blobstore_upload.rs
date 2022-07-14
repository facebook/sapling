/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::TryFutureExt;
use slog::info;
use slog::Logger;

use crate::error::SubcommandError;

pub const KEY_ARG: &str = "key";
pub const VALUE_FILE_ARG: &str = "value-file";
pub const BLOBSTORE_UPLOAD: &str = "blobstore-upload";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BLOBSTORE_UPLOAD)
        .about("uploads blobs to blobstore")
        .arg(
            Arg::with_name(KEY_ARG)
                .long(KEY_ARG)
                .takes_value(true)
                .required(true)
                .help("blobstore key to upload to"),
        )
        .arg(
            Arg::with_name(VALUE_FILE_ARG)
                .long(VALUE_FILE_ARG)
                .takes_value(true)
                .required(true)
                .help("file with value to write to blobstore"),
        )
}

pub async fn subcommand_blobstore_upload<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;

    let key = sub_m
        .value_of(KEY_ARG)
        .ok_or_else(|| anyhow!("{} is not set", KEY_ARG))?;
    let value_file = sub_m
        .value_of(VALUE_FILE_ARG)
        .ok_or_else(|| anyhow!("{} is not set", VALUE_FILE_ARG))?;
    let data = tokio::fs::read(value_file).map_err(Error::from).await?;
    info!(ctx.logger(), "writing {} bytes to blobstore", data.len());

    repo.blobstore()
        .put(&ctx, key.to_string(), BlobstoreBytes::from_bytes(data))
        .await?;

    Ok(())
}
