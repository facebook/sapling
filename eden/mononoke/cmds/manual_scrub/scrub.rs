/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use cloned::cloned;
use futures::{
    channel::mpsc,
    sink::SinkExt,
    stream::{Stream, TryStreamExt},
};

use blobstore::Blobstore;
use context::CoreContext;

async fn scrub_key<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    ctx: &CoreContext,
    key: String,
    mut success: mpsc::Sender<String>,
    mut missing: mpsc::Sender<String>,
    mut error: mpsc::Sender<(String, Error)>,
) -> Result<()> {
    let handle = {
        cloned!(ctx, key, blobstore);
        tokio::task::spawn(async move { blobstore.get(ctx, key).await })
    };
    let res = handle.await?;
    match res {
        Ok(None) => {
            missing.send(key).await?;
        }
        Err(e) => {
            error.send((key, e)).await?;
        }
        Ok(Some(_)) => {
            success.send(key).await?;
        }
    };
    Ok(())
}

pub async fn scrub<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    ctx: &CoreContext,
    keys: impl Stream<Item = Result<String>>,
    success: mpsc::Sender<String>,
    missing: mpsc::Sender<String>,
    error: mpsc::Sender<(String, Error)>,
    scheduled_max: usize,
) -> Result<()> {
    keys.try_for_each_concurrent(scheduled_max, |key| {
        scrub_key(
            blobstore,
            ctx,
            key,
            success.clone(),
            missing.clone(),
            error.clone(),
        )
    })
    .await?;
    Ok(())
}
