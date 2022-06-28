/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use cloned::cloned;
use futures::channel::mpsc;
use futures::future;
use futures::future::FutureExt;
use futures::sink::SinkExt;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use std::time::Instant;

use blobstore::Blobstore;
use context::CoreContext;

use crate::checkpoint::FileCheckpoint;
use crate::progress::Progress;
use crate::tracker::Tracker;

const PROGRESS_SAMPLE_KEYS: u64 = 1000;

async fn scrub_key<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    ctx: &CoreContext,
    key: String,
    success: Option<mpsc::Sender<String>>,
    mut missing: mpsc::Sender<String>,
    mut error: mpsc::Sender<(String, Error)>,
) -> Result<(Progress, String)> {
    let handle = {
        cloned!(ctx, key, blobstore);
        tokio::task::spawn(async move { blobstore.get(&ctx, &key).await })
    };
    let res = handle.await?;
    let mut progress = Progress::default();
    {
        cloned!(key);
        match res {
            Ok(None) => {
                missing.send(key).await?;
                progress.missing += 1;
            }
            Err(e) => {
                error.send((key, e)).await?;
                progress.error += 1;
            }
            Ok(Some(v)) => {
                if let Some(mut success) = success {
                    success.send(key).await?;
                }
                progress.success += 1;
                progress.bytes += v.as_bytes().len() as u64;
            }
        }
    };

    Ok((progress, key))
}

pub async fn scrub<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    ctx: &CoreContext,
    keys: impl Stream<Item = Result<String>>,
    success: Option<mpsc::Sender<String>>,
    missing: mpsc::Sender<String>,
    error: mpsc::Sender<(String, Error)>,
    checkpoint: Option<FileCheckpoint>,
    scheduled_max: usize,
    quiet: bool,
) -> Result<()> {
    let init = Progress::default();
    let started = Instant::now();
    if !quiet {
        init.legend(ctx.logger());
    }

    let mut starting_key = checkpoint
        .as_ref()
        .and_then(|cp| cp.read().transpose())
        .transpose()?;

    // keep a tracker so that we can use buffered_unordered but still checkpoint safely
    let tracker = Tracker::with_capacity(scheduled_max);
    let mut pos: u64 = 0;

    let (run, last_update, cp, last_key) = keys
        .map_ok(|key| {
            tracker.insert(key.clone(), pos);
            pos += 1;
            key
        })
        .map(|key| match key {
            Ok(key) => {
                if let Some(start) = starting_key.as_ref() {
                    if start == &key {
                        let _ = starting_key.take();
                    }
                    let mut progress = Progress::default();
                    progress.skipped += 1;
                    return future::ready(Ok((progress, key))).right_future();
                }
                scrub_key(
                    blobstore,
                    ctx,
                    key,
                    success.clone(),
                    missing.clone(),
                    error.clone(),
                )
                .left_future()
            }
            Err(e) => future::ready(Err(e)).right_future(),
        })
        .buffer_unordered(scheduled_max)
        .try_fold(
            (init, Some((init, started)), checkpoint, None),
            |(run, mut prev, checkpoint, _prev_key), (latest, key)| {
                let tracker = &tracker;
                async move {
                    tracker.mark_done(&key)?;
                    let run = run + latest;
                    // overkill to check time elapsed every key, so sample
                    if run.total() % PROGRESS_SAMPLE_KEYS == 0 {
                        if let Some(updated) =
                            run.record(ctx.logger(), quiet, started, prev, false)?
                        {
                            let best_done = tracker.compact();
                            match (best_done, checkpoint.as_ref()) {
                                (Some(done_key), Some(checkpoint)) if run.success > 0 => {
                                    checkpoint.update(ctx.logger(), &done_key)?;
                                }
                                _ => {}
                            }

                            prev = Some((run, updated));
                        }
                    }
                    Ok((run, prev, checkpoint, Some(key)))
                }
            },
        )
        .await?;

    // Record progress at finish
    run.record(ctx.logger(), quiet, started, last_update, true)?;

    // Record the last update
    if run.success > 0 {
        match (cp.as_ref(), last_key) {
            (Some(cp), Some(last_key)) => cp.update(ctx.logger(), &last_key)?,
            _ => {}
        }
    }
    Ok(())
}
