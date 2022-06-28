/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use crate::utils;
use crate::MononokeSQLBlobGCArgs;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_app::MononokeApp;
use retry::retry;
use slog::info;
use slog::Logger;

use sqlblob::Sqlblob;

const BASE_RETRY_DELAY_MS: u64 = 1000;
const RETRIES: usize = 3;

/// mark referenced blobs as not safe to delete
#[derive(Parser)]
pub struct CommandArgs {
    /// Only set generation on blobs that have no generation set yet. Do not do a full mark.
    #[clap(long)]
    initial_generation_only: bool,
    /// Only do the mark; do not set generation on blobs with no generation set yet.
    #[clap(long)]
    skip_initial_generation: bool,
    /// Only set the generation, don't inline small values
    #[clap(long)]
    skip_inline_small_values: bool,
}

async fn handle_one_key(
    key: String,
    store: Arc<Sqlblob>,
    inline_small_values: bool,
    logger: Arc<Logger>,
    mark_generation: u64,
) -> Result<()> {
    retry(
        &logger,
        |_| store.set_generation(&key, inline_small_values, mark_generation),
        BASE_RETRY_DELAY_MS,
        RETRIES,
    )
    .await
    .with_context(|| anyhow!("Failed to handle {} after {} retries", &key, RETRIES))?;
    Ok(())
}

async fn handle_initial_generation(store: &Sqlblob, shard: usize, logger: &Logger) -> Result<()> {
    retry(
        logger,
        |_| store.set_initial_generation(shard),
        BASE_RETRY_DELAY_MS,
        RETRIES,
    )
    .await
    .with_context(|| {
        anyhow!(
            "Failed to handle initial generation on shard {} after {} retries",
            &shard,
            RETRIES
        )
    })?;
    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let common_args: MononokeSQLBlobGCArgs = app.args()?;

    let logger: Logger = app.logger().clone();

    let max_parallelism: usize = common_args.scheduled_max;

    let (sqlblob, shard_range) = utils::get_sqlblob_and_shard_range(&app).await?;

    if !args.skip_initial_generation {
        info!(logger, "Starting initial generation set");
        let set_initial_generation_futures: Vec<_> = shard_range
            .clone()
            .map(|shard| Ok(handle_initial_generation(&sqlblob, shard, &logger)))
            .collect();
        stream::iter(set_initial_generation_futures.into_iter())
            .try_for_each_concurrent(max_parallelism, |fut| fut)
            .await?;
        info!(logger, "Completed initial generation set");
    }

    if args.initial_generation_only {
        return Ok(());
    }

    let sqlblob = Arc::new(sqlblob);
    let logger = Arc::new(logger);

    let inline_small_values = !args.skip_inline_small_values;

    // Hold mark generation constant for run
    let mark_generation = sqlblob.get_mark_generation();

    info!(logger, "Starting marking generation {}", mark_generation);
    // Set up a task to process each key in parallel in its own task.
    let (key_channel, processor) = {
        let sqlblob = Arc::clone(&sqlblob);
        let logger = Arc::clone(&logger);
        let (tx, rx) = mpsc::channel(10);
        let task = tokio::spawn(async move {
            rx.map(Ok)
                .try_for_each_concurrent(max_parallelism, {
                    |key| {
                        let sqlblob = sqlblob.clone();
                        let logger = logger.clone();
                        async move {
                            tokio::spawn(handle_one_key(
                                key,
                                sqlblob,
                                inline_small_values,
                                logger,
                                mark_generation,
                            ))
                            .await?
                        }
                    }
                })
                .await
        });
        (tx, task)
    };

    // Foreach shard in shard_range
    for shard in shard_range {
        info!(logger, "Starting mark on data keys from shard {}", shard);
        let res = sqlblob
            .get_keys_from_shard(shard)
            .forward(key_channel.clone().sink_err_into())
            .await;
        // Report processing errors ahead of key errors - that way, we don't lose the error if the channel goes away because of an error
        if res.is_err() {
            std::mem::drop(key_channel);
            processor.await??;
            return res;
        }
    }

    // Drop the spare sender so that the processor task can exit
    std::mem::drop(key_channel);

    processor.await??;
    info!(logger, "Completed marking generation {}", mark_generation);
    Ok(())
}
