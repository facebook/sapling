/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::OsStr;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_compression::tokio::write::ZstdEncoder;
use async_compression::Level;
use blobstore_factory::make_blobstore;
use blobstore_factory::ScrubAction;
use blobstore_factory::SrubWriteOnly;
use clap_old::Arg;
use cmdlib::args;
use cmdlib::args::ArgType;
use context::CoreContext;
use futures::channel::mpsc;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use slog::info;
use tokio::fs::File;
use tokio::io::stdin;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

mod checkpoint;
mod progress;
mod scrub;
mod tracker;

use crate::checkpoint::FileCheckpoint;
use crate::scrub::scrub;

const ARG_STORAGE_CONFIG_NAME: &str = "storage-config-name";
const ARG_SCHEDULED_MAX: &str = "scheduled-max";

const ARG_SUCCESSFUL_KEYS: &str = "success-keys-output";
const ARG_MISSING_KEYS: &str = "missing-keys-output";
const ARG_ERROR_KEYS: &str = "error-keys-output";
const ARG_CHECKPOINT_KEY: &str = "checkpoint-key-file";
const ARG_ZSTD_LEVEL: &str = "keys-zstd-level";
const ARG_QUIET: &str = "quiet";

async fn bridge_to_file<W: AsyncWriteExt + Unpin>(
    mut file: W,
    mut recv: mpsc::Receiver<String>,
) -> Result<()> {
    while let Some(string) = recv.next().await {
        file.write_all(string.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    let _ = file.shutdown().await;
    Ok(())
}

async fn handle_errors<W: AsyncWriteExt + Unpin>(
    mut file: W,
    mut recv: mpsc::Receiver<(String, Error)>,
) -> Result<()> {
    while let Some((key, err)) = recv.next().await {
        eprintln!("Error: {:?}", err.context(format!("Scrubbing key {}", key)));
        file.write_all(key.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    let _ = file.shutdown().await;
    Ok(())
}

async fn create_file(
    file_name: &OsStr,
    zstd_level: Option<i32>,
) -> Result<Box<dyn AsyncWrite + Unpin + Send>, Error> {
    let file = File::create(file_name).await?;
    if let Some(zstd_level) = zstd_level {
        Ok(Box::new(ZstdEncoder::with_quality(
            file,
            Level::Precise(zstd_level as u32),
        )))
    } else {
        Ok(Box::new(file))
    }
}

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<()> {
    let app = args::MononokeAppBuilder::new("manual scrub")
        .with_advanced_args_hidden()
        .with_all_repos()
        .with_arg_types(vec![ArgType::Scrub])
        .with_scrub_action_default(Some(ScrubAction::Repair))
        .with_scrub_action_on_missing_write_only_default(Some(SrubWriteOnly::Scrub))
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
                .required_unless(ARG_CHECKPOINT_KEY)
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
        )
        .arg(
            Arg::with_name(ARG_CHECKPOINT_KEY)
                .long(ARG_CHECKPOINT_KEY)
                .takes_value(true)
                .required(false)
                .help("A file to write checkpoint key to"),
        )
        .arg(
            Arg::with_name(ARG_ZSTD_LEVEL)
                .long(ARG_ZSTD_LEVEL)
                .takes_value(true)
                .required(false)
                .help("Pass a level to Zstd compress the output files, 0 means Zstd default"),
        )
        .arg(
            Arg::with_name(ARG_QUIET)
                .long(ARG_QUIET)
                .takes_value(false)
                .required(false)
                .help("Run quietly with minimal progress logging"),
        );

    let matches = app.get_matches(fb)?;
    let logger = matches.logger();
    let runtime = matches.runtime();
    let config_store = matches.config_store();

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

    let mysql_options = matches.mysql_options();
    let blobstore_options = matches.blobstore_options();
    let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());

    let success_file_name = matches.value_of_os(ARG_SUCCESSFUL_KEYS);
    let missing_keys_file_name = matches
        .value_of_os(ARG_MISSING_KEYS)
        .context("No missing data output file")?;
    let errors_file_name = matches
        .value_of_os(ARG_ERROR_KEYS)
        .context("No errored keys output file")?;
    let zstd_level = args::get_i32_opt(&matches, ARG_ZSTD_LEVEL);
    let quiet = matches.is_present(ARG_QUIET);
    let checkpoint = matches
        .value_of_os(ARG_CHECKPOINT_KEY)
        .map(FileCheckpoint::new);

    let scrub = async move {
        let blobstore = make_blobstore(
            fb,
            storage_config.blobstore,
            mysql_options,
            blobstore_factory::ReadOnlyStorage(false),
            blobstore_options,
            logger,
            config_store,
            &blobstore_factory::default_scrub_handler(),
            None,
        )
        .await?;

        if !quiet {
            info!(logger, "Scrubbing blobstore: {}", blobstore);
        }

        let stdin = BufReader::new(stdin());
        let mut output_handles = FuturesUnordered::new();
        let success = if let Some(success_file_name) = success_file_name {
            let (send, recv) = mpsc::channel(100);
            let file = create_file(success_file_name, zstd_level).await?;
            output_handles.push(tokio::spawn(bridge_to_file(file, recv)));
            Some(send)
        } else {
            None
        };
        let missing = {
            let (send, recv) = mpsc::channel(100);
            let file = create_file(missing_keys_file_name, zstd_level).await?;
            output_handles.push(tokio::spawn(bridge_to_file(file, recv)));
            send
        };
        let error = {
            let (send, recv) = mpsc::channel(100);
            let file = create_file(errors_file_name, zstd_level).await?;
            output_handles.push(tokio::spawn(handle_errors(file, recv)));
            send
        };

        let res = scrub(
            &blobstore,
            &ctx,
            tokio_stream::wrappers::LinesStream::new(stdin.lines()).map_err(Error::from),
            success,
            missing,
            error,
            checkpoint,
            scheduled_max,
            quiet,
        )
        .await
        .context("Scrub failed");

        while let Some(task_result) = output_handles.try_next().await? {
            task_result.context("Writing output files failed")?;
        }
        res
    };

    runtime.block_on(scrub)
}
