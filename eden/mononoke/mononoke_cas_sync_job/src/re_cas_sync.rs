/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use changesets_uploader::MononokeCasChangesetsUploader;
use context::CoreContext;
use slog::info;

use crate::CombinedBookmarkUpdateLogEntry;
use crate::Repo;
use crate::RetryAttemptsCount;

const DEFAULT_UPLOAD_RETRY_NUM: usize = 1;

/// Sends commits to CAS while syncing a set of bookmark update log entries.
pub async fn try_sync_single_combined_entry<'a>(
    _re_cas_client: &MononokeCasChangesetsUploader<'a>,
    _repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &CombinedBookmarkUpdateLogEntry,
) -> Result<RetryAttemptsCount, Error> {
    let ids: Vec<_> = combined_entry
        .components
        .iter()
        .map(|entry| entry.id)
        .collect();
    info!(ctx.logger(), "syncing log entries {:?} ...", ids);
    // The upload part is not implemented yet.
    // we need to fetch a list of commits from Commit Graph for those
    // entries and upload them to CAS
    Err(anyhow!("Not implemented yet"))
    // Ok(RetryAttemptsCount(DEFAULT_UPLOAD_RETRY_NUM))
}
