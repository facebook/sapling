/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use context::CoreContext;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_retry::retry;
use mononoke_app::MononokeApp;
use mononoke_macros::mononoke;
use slog::Logger;
use slog::info;
use sqlblob::Sqlblob;

use crate::MononokeSQLBlobGCArgs;
use crate::utils;

const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);
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
    ctx: CoreContext,
    key: String,
    store: Arc<Sqlblob>,
    inline_small_values: bool,
    logger: Arc<Logger>,
    mark_generation: u64,
) -> Result<()> {
    retry(
        |_| store.set_generation(&ctx, &key, inline_small_values, mark_generation),
        BASE_RETRY_DELAY,
    )
    .binary_exponential_backoff()
    .max_attempts(RETRIES)
    .inspect_err(|attempt, _err| info!(logger, "attempt {attempt} of {RETRIES} failed"))
    .await
    .with_context(|| anyhow!("Failed to handle {key} after {RETRIES} attempts"))?;
    Ok(())
}

async fn handle_initial_generation(
    ctx: &CoreContext,
    store: &Sqlblob,
    shard: usize,
    logger: &Logger,
) -> Result<()> {
    retry(
        |_| store.set_initial_generation(ctx, shard),
        BASE_RETRY_DELAY,
    )
    .binary_exponential_backoff()
    .max_attempts(RETRIES)
    .inspect_err(|attempt, _err| info!(logger, "attempt {attempt} of {RETRIES} failed"))
    .await
    .with_context(|| {
        anyhow!("Failed to handle initial generation on shard {shard} after {RETRIES} retries",)
    })?;
    info!(
        logger,
        "Completed initial generation handling on shard {}", shard
    );
    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let common_args: MononokeSQLBlobGCArgs = app.args()?;
    let ctx = app.new_basic_context();

    let logger: Logger = app.logger().clone();

    let max_parallelism: usize = common_args.scheduled_max;

    let (sqlblob, shard_range) = utils::get_sqlblob_and_shard_range(&app).await?;

    if !args.skip_initial_generation {
        info!(logger, "Starting initial generation set");
        let set_initial_generation_futures: Vec<_> = shard_range
            .clone()
            .map(|shard| Ok(handle_initial_generation(&ctx, &sqlblob, shard, &logger)))
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
        let new_ctx = ctx.clone();
        let task = mononoke::spawn_task(async move {
            rx.map(Ok)
                .try_for_each_concurrent(max_parallelism, {
                    |key| {
                        let sqlblob = sqlblob.clone();
                        let logger = logger.clone();
                        let new_ctx = new_ctx.clone();
                        async move {
                            mononoke::spawn_task(handle_one_key(
                                new_ctx,
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
            .get_keys_from_shard(&ctx, shard)
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
