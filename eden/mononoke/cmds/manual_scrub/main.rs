/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Context, Error, Result};
use clap::Arg;
use futures::{
    channel::mpsc,
    stream::{StreamExt, TryStreamExt},
};
use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader};

use cmdlib::args;
use context::CoreContext;

mod blobstore;
mod scrub;

use crate::{blobstore::open_blobstore, scrub::scrub};

const ARG_STORAGE_CONFIG_NAME: &str = "storage-config-name";

async fn bridge_to_stdout(mut recv: mpsc::Receiver<String>) -> Result<()> {
    let mut stdout = stdout();
    while let Some(string) = recv.next().await {
        stdout.write_all(string.as_bytes()).await?;
        stdout.write(b"\n").await?;
    }
    Ok(())
}

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<()> {
    let app = args::MononokeApp::new("manual scrub")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .arg(
            Arg::with_name(ARG_STORAGE_CONFIG_NAME)
                .long(ARG_STORAGE_CONFIG_NAME)
                .takes_value(true)
                .required(true)
                .help("the name of the storage config to scrub"),
        );
    let matches = app.get_matches();
    let (_, logger, mut runtime) =
        args::init_mononoke(fb, &matches, None).context("failed to initialise mononoke")?;

    let storage_config = args::load_storage_configs(fb, &matches)
        .context("Could not read storage configs")?
        .storage
        .remove(
            matches
                .value_of(ARG_STORAGE_CONFIG_NAME)
                .context("No storage config name")?,
        )
        .context("Requested storage config not found")?;

    let mysql_options = args::parse_mysql_options(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let ctx = CoreContext::new_bulk_with_logger(fb, logger.clone());

    let scrub = async move {
        let blobstore = open_blobstore(
            fb,
            storage_config,
            mysql_options,
            &blobstore_options,
            &logger,
        )
        .await?;

        let stdin = BufReader::new(stdin());
        let (output, recv) = mpsc::channel(100);
        let output_handle = tokio::spawn(bridge_to_stdout(recv));
        let res = scrub(
            &*blobstore,
            &ctx,
            stdin.lines().map_err(Error::from),
            output,
        )
        .await
        .context("Scrub failed");

        output_handle.await?.context("Writing to stdout failed")?;
        res
    };

    runtime.block_on_std(scrub)
}
