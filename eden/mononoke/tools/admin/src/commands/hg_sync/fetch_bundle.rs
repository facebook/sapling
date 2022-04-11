/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use bookmarks::{BookmarkUpdateLogRef, Freshness};
use clap::Args;
use context::CoreContext;
use futures::stream::StreamExt;
use mercurial_bundle_replay_data::BundleReplayData;
use mononoke_hg_sync_job_helper_lib::save_bundle_to_file;
use repo_blobstore::RepoBlobstoreRef;

use super::Repo;

#[derive(Args)]
pub struct HgSyncFetchBundleArgs {
    /// Sync log entry to fetch the bundle for
    id: i64,

    /// Output file to write the bundle to
    #[clap(long, short = 'o', value_name = "FILE", parse(from_os_str))]
    output: PathBuf,
}

pub async fn fetch_bundle(
    ctx: &CoreContext,
    repo: &Repo,
    fetch_bundle_args: HgSyncFetchBundleArgs,
) -> Result<()> {
    let log_entry = repo
        .bookmark_update_log()
        .read_next_bookmark_log_entries(
            ctx.clone(),
            (fetch_bundle_args.id - 1)
                .try_into()
                .context("Invalid log id")?,
            1,
            Freshness::MostRecent,
        )
        .next()
        .await
        .ok_or_else(|| anyhow!("No log entries found"))??;

    if log_entry.id != fetch_bundle_args.id {
        bail!("No entry with id {} found", fetch_bundle_args.id);
    }

    let bundle_replay_data: BundleReplayData = log_entry
        .bundle_replay_data
        .ok_or_else(|| anyhow!("No bundle found"))?
        .try_into()?;

    save_bundle_to_file(
        ctx,
        repo.repo_blobstore(),
        bundle_replay_data.bundle2_id,
        fetch_bundle_args.output,
        true, /* create */
    )
    .await
    .context("Failed to fetch bundle to file")?;

    Ok(())
}
