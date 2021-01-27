/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{ops::Range, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use clap::{App, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::{
    channel::mpsc,
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use rand::{thread_rng, Rng};
use slog::Logger;
use tokio::time::delay_for;

use sqlblob::Sqlblob;

pub const MARK_SAFE: &str = "mark";
const MIN_RETRY_DELAY: Duration = Duration::from_millis(10);
const MAX_RETRY_DELAY: Duration = Duration::from_millis(100);
const RETRIES: usize = 3;

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(MARK_SAFE).about("mark referenced blobs as not safe to delete")
}

async fn handle_one_key(key: String, store: Arc<Sqlblob>) -> Result<()> {
    for _retry in 0..RETRIES {
        let res = store.set_generation(&key).await;
        if res.is_ok() {
            return res;
        }
        let delay = thread_rng().gen_range(MIN_RETRY_DELAY, MAX_RETRY_DELAY);
        eprintln!("Failure {:#?} - delaying for {:#?}", res, delay);
        delay_for(delay).await;
    }
    Err(anyhow!("Did not make an attempt to handle {}", &key))
}

pub async fn subcommand_mark<'a>(
    _fb: FacebookInit,
    _logger: Logger,
    _sub_matches: &'a ArgMatches<'_>,
    max_parallelism: usize,
    sqlblob: Sqlblob,
    shard_range: Range<usize>,
) -> Result<()> {
    let sqlblob = Arc::new(sqlblob);

    // Set up a task to process each key in parallel in its own task.
    let (key_channel, processor) = {
        let sqlblob = Arc::clone(&sqlblob);
        let (tx, rx) = mpsc::channel(10);
        let task = tokio::spawn(async move {
            rx.map(Ok)
                .try_for_each_concurrent(max_parallelism, {
                    |key| {
                        let sqlblob = sqlblob.clone();
                        async move { tokio::spawn(handle_one_key(key, sqlblob)).await? }
                    }
                })
                .await
        });
        (tx, task)
    };

    // Foreach shard in shard_range
    for shard in shard_range {
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

    processor.await?
}
