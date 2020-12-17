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
    stream::{FuturesUnordered, StreamExt, TryStreamExt},
};
use tokio::{
    fs::File,
    io::{stdin, AsyncBufReadExt, AsyncWriteExt, BufReader},
};

use cmdlib::args;
use context::CoreContext;

mod blobstore;
mod scrub;

use crate::{blobstore::open_blobstore, scrub::scrub};

const ARG_STORAGE_CONFIG_NAME: &str = "storage-config-name";
const ARG_SCHEDULED_MAX: &str = "scheduled-max";

const ARG_SUCCESSFUL_KEYS: &str = "success-keys-output";
const ARG_MISSING_KEYS: &str = "missing-keys-output";
const ARG_ERROR_KEYS: &str = "error-keys-output";

async fn bridge_to_file(mut file: File, mut recv: mpsc::Receiver<String>) -> Result<()> {
    while let Some(string) = recv.next().await {
        file.write_all(string.as_bytes()).await?;
        file.write(b"\n").await?;
    }
    // Best effort to flush
    let _ = file.flush().await;
    Ok(())
}

async fn handle_errors(mut file: File, mut recv: mpsc::Receiver<(String, Error)>) -> Result<()> {
    while let Some((key, err)) = recv.next().await {
        eprintln!("Error: {:?}", err.context(format!("Scrubbing key {}", key)));
        file.write_all(key.as_bytes()).await?;
        file.write(b"\n").await?;
    }
    // Best effort to flush
    let _ = file.flush().await;
    Ok(())
}

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<()> {
    let app = args::MononokeAppBuilder::new("manual scrub")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .arg(
            Arg::with_name(ARG_STORAGE_CONFIG_NAME)
                .long(ARG_STORAGE_CONFIG_NAME)
                .takes_value(true)
                .required(true)
                .help("the name of the storage config to scrub"),
        )
        .arg(
            Arg::with_name(ARG_SCHEDULED_MAX)
                .long(ARG_SCHEDULED_MAX)
                .takes_value(true)
                .required(false)
                .help("Maximum number of scrub keys to attempt to execute at once.  Default 100."),
        )
        .arg(
            Arg::with_name(ARG_SUCCESSFUL_KEYS)
                .long(ARG_SUCCESSFUL_KEYS)
                .takes_value(true)
                .required(true)
                .help("A file to write successfully scrubbed key IDs to"),
        )
        .arg(
            Arg::with_name(ARG_MISSING_KEYS)
                .long(ARG_MISSING_KEYS)
                .takes_value(true)
                .required(true)
                .help("A file to write missing data key IDs to"),
        )
        .arg(
            Arg::with_name(ARG_ERROR_KEYS)
                .long(ARG_ERROR_KEYS)
                .takes_value(true)
                .required(true)
                .help("A file to write error fetching data key IDs to"),
        );

    let matches = app.get_matches();
    let (_, logger, mut runtime) =
        args::init_mononoke(fb, &matches).context("failed to initialise mononoke")?;
    let config_store = args::init_config_store(fb, &logger, &matches)?;

    let scheduled_max = args::get_usize_opt(&matches, ARG_SCHEDULED_MAX).unwrap_or(100) as usize;

    let storage_config = args::load_storage_configs(config_store, &matches)
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

    let success_file_name = matches
        .value_of_os(ARG_SUCCESSFUL_KEYS)
        .context("No successfully scrubbed output file")?;
    let missing_keys_file_name = matches
        .value_of_os(ARG_MISSING_KEYS)
        .context("No missing data output file")?;
    let errors_file_name = matches
        .value_of_os(ARG_ERROR_KEYS)
        .context("No errored keys output file")?;

    let scrub = async move {
        let blobstore = open_blobstore(
            fb,
            storage_config,
            &mysql_options,
            &blobstore_options,
            &logger,
            config_store,
        )
        .await?;

        let stdin = BufReader::new(stdin());
        let mut output_handles = FuturesUnordered::new();
        let success = {
            let (send, recv) = mpsc::channel(100);
            let file = File::create(success_file_name).await?;
            output_handles.push(tokio::spawn(bridge_to_file(file, recv)));
            send
        };
        let missing = {
            let (send, recv) = mpsc::channel(100);
            let file = File::create(missing_keys_file_name).await?;
            output_handles.push(tokio::spawn(bridge_to_file(file, recv)));
            send
        };
        let error = {
            let (send, recv) = mpsc::channel(100);
            let file = File::create(errors_file_name).await?;
            output_handles.push(tokio::spawn(handle_errors(file, recv)));
            send
        };
        let res = scrub(
            &blobstore,
            &ctx,
            stdin.lines().map_err(Error::from),
            success,
            missing,
            error,
            scheduled_max,
        )
        .await
        .context("Scrub failed");

        while let Some(task_result) = output_handles.try_next().await? {
            task_result.context("Writing output files failed")?;
        }
        res
    };

    runtime.block_on_std(scrub)
}
