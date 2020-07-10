/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use futures::{
    channel::mpsc,
    sink::SinkExt,
    stream::{Stream, TryStreamExt},
};

use blobstore::Blobstore;
use context::CoreContext;

async fn scrub_key(
    blobstore: &dyn Blobstore,
    ctx: &CoreContext,
    key: String,
    mut output: mpsc::Sender<String>,
) -> Result<()> {
    blobstore
        .get(ctx.clone(), key.clone())
        .await?
        .with_context(|| format!("Key {} is missing", &key))?;
    output.send(key).await?;
    Ok(())
}

pub async fn scrub(
    blobstore: &dyn Blobstore,
    ctx: &CoreContext,
    keys: impl Stream<Item = Result<String>>,
    output: mpsc::Sender<String>,
) -> Result<()> {
    keys.try_for_each_concurrent(100, |key| scrub_key(blobstore, ctx, key, output.clone()))
        .await?;
    Ok(())
}
