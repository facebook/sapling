/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use bookmarks::BookmarkUpdateLogEntry;
use context::CoreContext;
use edenapi_types::AnyId;
use http_client::Stats;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::define_stats;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.modern_sync.stats";

    bookmark_update_entry_done_count: dynamic_timeseries("{}.bookmark_update_log.processed.count", (repo: String); Sum),
    bookmark_update_entry_segments_done_count: dynamic_timeseries("{}.bul.segments.time_ms", (repo: String); Sum),
    bookmark_update_entry_done_time_ms: dynamic_timeseries("{}.bookmark_update_log.processed.time_ms", (repo: String); Average),
    bookmark_update_entry_error_count: dynamic_timeseries("{}.bookmark_update_log.processed.error.count", (repo: String); Sum),
}

pub(crate) fn new(
    app: Arc<MononokeApp>,
    metadata: &Metadata,
    repo_name: &str,
    dry_run: bool,
) -> MononokeScubaSampleBuilder {
    let uuid = uuid::Uuid::new_v4();
    let mut scuba_sample = app.environment().scuba_sample_builder.clone();
    scuba_sample.add_metadata(metadata);
    scuba_sample.add("run_id", uuid.to_string());
    scuba_sample.add("repo", repo_name);
    scuba_sample.add("dry_run", dry_run);

    scuba_sample
}

pub(crate) fn log_sync_start(ctx: &CoreContext, start_id: u64) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start sync process");
    scuba_sample.add("start_id", start_id);

    scuba_sample.log()
}

pub(crate) fn log_bookmark_update_entry_start(
    ctx: &CoreContext,
    entry: &BookmarkUpdateLogEntry,
    approx_count: Option<i64>,
) -> (bool, CoreContext) {
    let mut scuba_sample = ctx.scuba().clone();
    let ctx = ctx.with_mutated_scuba(|mut scuba| {
        scuba.add("bookmark_name", entry.bookmark_name.as_str());
        scuba
    });

    scuba_sample.add("log_tag", "Start processing bookmark update entry");
    add_bookmark_entry_info(&mut scuba_sample, entry);
    if let Some(count) = approx_count {
        scuba_sample.add("bookmark_entry_commits_count", count);
    }

    let res = scuba_sample.log();
    (res, ctx)
}

pub(crate) fn log_bookmark_update_entry_segments_done(
    ctx: &CoreContext,
    repo_name: &str,
    latest_checkpoint: i64,
    elapsed: std::time::Duration,
) {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Done calculating bookmark update entry segments");
    scuba_sample.add("latest_checkpoint", latest_checkpoint);
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log();

    STATS::bookmark_update_entry_segments_done_count
        .add_value(elapsed.as_millis() as i64, (repo_name.to_string(),));
}

pub(crate) fn log_bookmark_update_entry_done(
    ctx: &CoreContext,
    repo_name: &str,
    entry: &BookmarkUpdateLogEntry,
    elapsed: std::time::Duration,
) {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Done processing bookmark update entry");
    add_bookmark_entry_info(&mut scuba_sample, entry);
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log();

    STATS::bookmark_update_entry_done_count.add_value(1, (repo_name.to_string(),));
    STATS::bookmark_update_entry_done_time_ms
        .add_value(elapsed.as_millis() as i64, (repo_name.to_string(),));
}

pub(crate) fn add_bookmark_entry_info(
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

pub(crate) fn log_bookmark_update_entry_error(
    ctx: &CoreContext,
    repo_name: &str,
    entry: &BookmarkUpdateLogEntry,
    error: &anyhow::Error,
    elapsed: std::time::Duration,
) {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Error processing bookmark");
    add_bookmark_entry_info(&mut scuba_sample, entry);
    scuba_sample.add("error", format!("{:?}", error));
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log();

    STATS::bookmark_update_entry_done_count.add_value(1, (repo_name.to_string(),));
    STATS::bookmark_update_entry_done_time_ms
        .add_value(elapsed.as_millis() as i64, (repo_name.to_string(),));
    STATS::bookmark_update_entry_error_count.add_value(1, (repo_name.to_string(),));
}

pub(crate) fn log_changeset_start(ctx: &CoreContext, changeset_id: &ChangesetId) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start processing changeset");
    add_changeset_info(&mut scuba_sample, changeset_id);

    scuba_sample.log()
}

pub(crate) fn log_changeset_done(
    ctx: &CoreContext,
    changeset_id: &ChangesetId,
    elapsed: std::time::Duration,
) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Done processing changeset");
    add_changeset_info(&mut scuba_sample, changeset_id);
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log()
}

pub(crate) fn log_changeset_error(
    ctx: &CoreContext,
    changeset_id: &ChangesetId,
    error: &anyhow::Error,
    elapsed: std::time::Duration,
) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Error processing changeset");
    add_changeset_info(&mut scuba_sample, changeset_id);
    scuba_sample.add("error", format!("{:?}", error));
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log()
}

fn add_changeset_info(scuba_sample: &mut MononokeScubaSampleBuilder, changeset_id: &ChangesetId) {
    scuba_sample.add("changeset_id", format!("{}", changeset_id.to_hex()));
}

pub(crate) fn log_edenapi_stats(
    mut scuba: MononokeScubaSampleBuilder,
    stats: &Stats,
    endpoint: &str,
    contents: Vec<AnyId>,
) {
    let mut contents = contents
        .iter()
        .map(|id| format!("{:?}", id))
        .collect::<Vec<_>>();
    contents.sort();

    scuba.add("log_tag", "EdenAPI stats");
    scuba.add("contents", contents);
    scuba.add("endpoint", endpoint);
    scuba.add("requests", stats.requests);
    // Bytes
    scuba.add("downloaded_bytes", stats.downloaded);
    scuba.add("uploaded_bytes", stats.uploaded);
    // Milliseconds
    scuba.add(
        "elapsed",
        u64::try_from(stats.time.as_millis()).unwrap_or(u64::MAX),
    );
    // Milliseconds
    scuba.add(
        "latency",
        u64::try_from(stats.latency.as_millis()).unwrap_or(u64::MAX),
    );
    // Compute the speed in MB/s
    let time = stats.time.as_millis() as f64 / 1000.0;
    let size = stats.downloaded as f64 / 1024.0 / 1024.0;
    scuba.add("download_speed", format!("{:.2}", size / time).as_str());
    let size = stats.uploaded as f64 / 1024.0 / 1024.0;
    scuba.add("upload_speed", format!("{:.2}", size / time).as_str());
    scuba.log();
}

pub(crate) fn log_upload_changeset_start(
    ctx: &CoreContext,
    changeset_ids: Vec<ChangesetId>,
) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start uploading changesets");
    add_changesets_info(&mut scuba_sample, changeset_ids);

    scuba_sample.log()
}

pub(crate) fn log_upload_changeset_done(
    ctx: &CoreContext,
    changeset_ids: Vec<ChangesetId>,
    elapsed: std::time::Duration,
) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Done uploading changesets");
    add_changesets_info(&mut scuba_sample, changeset_ids);
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log()
}

pub(crate) fn log_upload_changeset_error(
    ctx: &CoreContext,
    changeset_ids: Vec<ChangesetId>,
    error: &anyhow::Error,
    elapsed: std::time::Duration,
) -> bool {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Error uploading changesets");
    add_changesets_info(&mut scuba_sample, changeset_ids);
    scuba_sample.add("error", format!("{:?}", error));
    scuba_sample.add("elapsed", elapsed.as_millis());

    scuba_sample.log()
}

fn add_changesets_info(
    scuba_sample: &mut MononokeScubaSampleBuilder,
    changeset_ids: Vec<ChangesetId>,
) {
    scuba_sample.add(
        "changeset_ids",
        changeset_ids.iter().map(|c| c.to_hex()).collect::<Vec<_>>(),
    );
}
