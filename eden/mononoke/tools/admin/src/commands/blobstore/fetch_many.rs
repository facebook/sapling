/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use clap::Args;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;

#[derive(Args)]
pub struct BlobstoreFetchManyArgs {
    /// File with whitespace separated keys
    #[clap(long)]
    keys_file: String,

    /// How many fetches to do concurrently
    #[clap(long, default_value_t = 50)]
    concurrency: usize,
}

#[derive(Default)]
struct Stats {
    present: usize,
    missing: usize,
    failed: usize,
}

impl Stats {
    fn present() -> Self {
        Self {
            present: 1,
            missing: 0,
            failed: 0,
        }
    }
    fn missing() -> Self {
        Self {
            present: 0,
            missing: 1,
            failed: 0,
        }
    }
    fn failed() -> Self {
        Self {
            present: 0,
            missing: 0,
            failed: 1,
        }
    }
}

impl std::iter::Sum for Stats {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut ans = Self::default();
        for stat in iter {
            ans.present += stat.present;
            ans.missing += stat.missing;
            ans.failed += stat.failed;
        }
        ans
    }
}

pub async fn fetch_many(
    ctx: &CoreContext,
    blobstore: &dyn Blobstore,
    args: BlobstoreFetchManyArgs,
) -> Result<()> {
    let text = std::fs::read_to_string(args.keys_file).context("Reading keys file")?;
    let keys = text.split_whitespace();
    let stats: Stats = stream::iter(keys)
        .map(|key| async move {
            match blobstore.get(ctx, key).await {
                Err(_) => Stats::failed(),
                Ok(Some(_)) => Stats::present(),
                Ok(None) => Stats::missing(),
            }
        })
        // Prevents compiler bug
        .boxed()
        .buffer_unordered(args.concurrency)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .sum();

    println!(
        "present: {}\nmissing: {}\nfailed: {}",
        stats.present, stats.missing, stats.failed
    );

    Ok(())
}
