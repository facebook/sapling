/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use cross_repo_sync::CommitSyncerArgs;
use futures_stats::FutureStats;
use mononoke_types::ChangesetId;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{error, info, Logger};
use synced_commit_mapping::SyncedCommitMapping;

pub const SCUBA_TABLE: &'static str = "mononoke_x_repo_sync";

const SOURCE_REPO: &'static str = "source_repo";
const TARGET_REPO: &'static str = "target_repo";
const SOURCE_CS_ID: &'static str = "source_cs_id";
const SYNC_TYPE_ARG: &str = "sync_type";
const TARGET_CS_ID: &'static str = "target_cs_id";
const DURATION_MS: &'static str = "duration_ms";
const ERROR: &'static str = "error";
const SUCCESS: &'static str = "success";

/// Populate the `scuba_sample` with fields, common for
/// this tailer run
pub fn add_common_fields<M: SyncedCommitMapping + Clone + 'static>(
    scuba_sample: &mut ScubaSampleBuilder,
    commit_syncer_args: &CommitSyncerArgs<M>,
) {
    scuba_sample
        .add(SOURCE_REPO, commit_syncer_args.get_source_repo_id().id())
        .add(TARGET_REPO, commit_syncer_args.get_target_repo_id().id());
}

/// Log the fact of successful syncing of the single changeset to Scuba
fn log_success_to_scuba(
    mut scuba_sample: ScubaSampleBuilder,
    source_cs_id: ChangesetId,
    maybe_synced_cs_id: Option<ChangesetId>,
    stats: FutureStats,
) {
    scuba_sample
        .add(DURATION_MS, stats.completion_time.as_millis() as u64)
        .add(SUCCESS, 1)
        .add(SOURCE_CS_ID, format!("{}", source_cs_id));
    if let Some(cs_id) = maybe_synced_cs_id {
        // Not producing changeset in a target repo is possible,
        // when syncing just dropped all the changes in the commit
        scuba_sample.add(TARGET_CS_ID, format!("{}", cs_id));
    }
    scuba_sample.log();
}

/// Log the fact of failed syncing of the single changeset to Scuba
fn log_error_to_scuba(
    mut scuba_sample: ScubaSampleBuilder,
    source_cs_id: ChangesetId,
    stats: FutureStats,
    error_string: String,
) {
    scuba_sample.add(SUCCESS, 0).add(ERROR, error_string);
    scuba_sample.add(DURATION_MS, stats.completion_time.as_millis() as u64);
    scuba_sample.add(SOURCE_CS_ID, format!("{}", source_cs_id));
    scuba_sample.log();
}

fn log_success_to_logger(
    logger: &Logger,
    source_cs_id: &ChangesetId,
    maybe_synced_cs_id: &Option<ChangesetId>,
    stats: &FutureStats,
) {
    let duration = stats.completion_time.as_millis();
    match maybe_synced_cs_id {
        Some(synced_cs_id) => {
            info!(
                logger,
                "changeset {} synced as {} in {}ms", source_cs_id, synced_cs_id, duration,
            );
        }
        None => {
            info!(
                logger,
                "Syncing {} succeeded in {}ms but did not produce a changeset in the taret repo.",
                source_cs_id,
                duration,
            );
        }
    };
}

fn log_error_to_logger(
    logger: &Logger,
    action: &'static str,
    source_cs_id: &ChangesetId,
    stats: &FutureStats,
    error_string: &String,
) {
    let duration = stats.completion_time.as_millis();
    error!(
        logger,
        "{} {} failed in {}ms: {}", action, source_cs_id, duration, error_string
    );
}

/// Log the fact of syncing of a single changeset both to Scuba and to slog
fn log_sync_single_changeset_result(
    ctx: CoreContext,
    scuba_sample: ScubaSampleBuilder,
    bcs_id: ChangesetId,
    res: &Result<Option<ChangesetId>, Error>,
    stats: FutureStats,
) {
    match res {
        Ok(maybe_synced_cs_id) => {
            log_success_to_logger(ctx.logger(), &bcs_id, &maybe_synced_cs_id, &stats);
            log_success_to_scuba(scuba_sample, bcs_id, *maybe_synced_cs_id, stats);
        }
        Err(e) => {
            let es = format!("{}", e);
            log_error_to_logger(ctx.logger(), "Syncing", &bcs_id, &stats, &es);
            log_error_to_scuba(scuba_sample, bcs_id, stats, es);
        }
    }
}

pub fn log_pushrebase_sync_single_changeset_result(
    ctx: CoreContext,
    mut scuba_sample: ScubaSampleBuilder,
    bcs_id: ChangesetId,
    res: &Result<Option<ChangesetId>, Error>,
    stats: FutureStats,
) {
    scuba_sample.add(SYNC_TYPE_ARG, "pushrebase");
    log_sync_single_changeset_result(ctx, scuba_sample, bcs_id, &res, stats)
}

pub fn log_non_pushrebase_sync_single_changeset_result(
    ctx: CoreContext,
    mut scuba_sample: ScubaSampleBuilder,
    bcs_id: ChangesetId,
    res: &Result<Option<ChangesetId>, Error>,
    stats: FutureStats,
) {
    scuba_sample.add(SYNC_TYPE_ARG, "non-pushrebase");
    log_sync_single_changeset_result(ctx, scuba_sample, bcs_id, res, stats)
}

pub fn log_bookmark_deletion_result(
    mut scuba_sample: ScubaSampleBuilder,
    res: &Result<(), Error>,
    stats: FutureStats,
) {
    scuba_sample.add(SYNC_TYPE_ARG, "bookmark_deletion");
    match res {
        Ok(()) => {
            scuba_sample.add(SUCCESS, 1);
        }
        Err(ref err) => {
            scuba_sample.add(SUCCESS, 0).add(ERROR, format!("{}", err));
        }
    }
    scuba_sample.add(DURATION_MS, stats.completion_time.as_millis() as u64);
    scuba_sample.log();
}

pub fn log_backpressure(ctx: &CoreContext, entries: u64, mut scuba_sample: ScubaSampleBuilder) {
    let msg = format!("{} entries in backsyncer queue, waiting...", entries);

    info!(ctx.logger(), "{}", msg);
    scuba_sample.log_with_msg("Backpressure", Some(msg));
}

pub fn log_bookmark_update_result(
    ctx: &CoreContext,
    entry_id: i64,
    mut scuba_sample: ScubaSampleBuilder,
    res: &Result<Vec<ChangesetId>, Error>,
    stats: FutureStats,
) {
    scuba_sample.add(DURATION_MS, stats.completion_time.as_millis() as u64);
    match res {
        Ok(_) => {
            info!(
                ctx.logger(),
                "successful sync bookmark update log #{}", entry_id
            );
            scuba_sample.add(SUCCESS, 1);
            scuba_sample.log();
        }
        Err(ref err) => {
            error!(
                ctx.logger(),
                "failed to sync bookmark update log #{}, {}", entry_id, err
            );
            scuba_sample.add(SUCCESS, 0);
            scuba_sample.add(ERROR, format!("{}", err));
            scuba_sample.log();
        }
    }
}

/// Log a Scuba sample, which will allow us to check that the tailer
/// is not dead, even when no commits are being synced
pub fn log_noop_iteration(mut scuba_sample: ScubaSampleBuilder) {
    scuba_sample.add(SUCCESS, 1);
    scuba_sample.log();
}
