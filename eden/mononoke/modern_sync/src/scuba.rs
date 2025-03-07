/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkUpdateLogEntry;
use context::CoreContext;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use scuba_ext::MononokeScubaSampleBuilder;

pub(crate) fn new(
    app: Arc<MononokeApp>,
    metadata: &Metadata,
    repo_name: &str,
    dry_run: bool,
) -> Result<MononokeScubaSampleBuilder> {
    let uuid = uuid::Uuid::new_v4();
    let mut scuba_sample = app.environment().scuba_sample_builder.clone();
    scuba_sample.add_metadata(metadata);
    scuba_sample.add("run_id", uuid.to_string());
    scuba_sample.add("repo", repo_name);
    scuba_sample.add("dry_run", dry_run);

    Ok(scuba_sample)
}

pub(crate) fn log_sync_start(ctx: &CoreContext, start_id: u64) -> Result<bool> {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start sync process");
    scuba_sample.add("start_id", start_id);

    Ok(scuba_sample.log())
}

pub(crate) fn log_bookmark_update_entry_start(
    ctx: &CoreContext,
    entry: &BookmarkUpdateLogEntry,
    commits_count: usize,
) -> Result<bool> {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start processing bookmark update entry");
    scuba_sample.add("bookmark_entry_commits_count", commits_count);
    add_bookmark_entry(&mut scuba_sample, entry);

    Ok(scuba_sample.log())
}

pub(crate) fn log_bookmark_update_entry_done(
    ctx: &CoreContext,
    entry: &BookmarkUpdateLogEntry,
    elapsed: std::time::Duration,
) -> Result<bool> {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Done processing bookmark update entry");
    add_bookmark_entry(&mut scuba_sample, entry);
    scuba_sample.add("elapsed", elapsed.as_secs());

    Ok(scuba_sample.log())
}

fn add_bookmark_entry(
    scuba_sample: &mut MononokeScubaSampleBuilder,
    entry: &BookmarkUpdateLogEntry,
) {
    scuba_sample.add("bookmark_entry_id", u64::from(entry.id));
    scuba_sample.add("bookmark_entry_bookmark_name", entry.bookmark_name.as_str());
    if let Some(cs_id) = entry.from_changeset_id {
        scuba_sample.add("bookmark_entry_from_changeset_id", format!("{}", cs_id));
    }
    if let Some(cs_id) = entry.to_changeset_id {
        scuba_sample.add("bookmark_entry_to_changeset_id", format!("{}", cs_id));
    }
    scuba_sample.add("bookmark_entry_reason", format!("{}", entry.reason));
    scuba_sample.add("bookmark_entry_timestamp", entry.timestamp.since_seconds());
}
