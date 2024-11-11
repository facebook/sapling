/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogId;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use futures::StreamExt;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use slog::info;

use crate::bul_util;
use crate::Repo;
const MODERN_SYNC_COUNTER_NAME: &str = "modern_sync";

pub async fn sync(
    app: Arc<MononokeApp>,
    start_id_arg: Option<u64>,
    repo_arg: RepoArg,
) -> Result<()> {
    let repo: Repo = app.open_repo(&repo_arg).await?;
    let _repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name().to_string();

    let ctx = CoreContext::new_with_logger_and_client_info(
        app.fb,
        app.logger().clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
    )
    .clone_with_repo_name(&repo_name);

    let start_id = if let Some(id) = start_id_arg {
        id
    } else {
        repo.mutable_counters()
            .get_counter(&ctx, MODERN_SYNC_COUNTER_NAME)
            .await?
            .map(|val| val.try_into())
            .transpose()?
            .ok_or_else(|| {
                format_err!(
                    "No start-id or mutable counter {} provided",
                    MODERN_SYNC_COUNTER_NAME
                )
            })?
    };

    // TODO: Implement tailing mode for entries
    let entries = bul_util::get_one_entry(
        &ctx,
        repo.bookmark_update_log_arc(),
        BookmarkUpdateLogId(start_id),
    )
    .await;

    info!(
        app.logger(),
        "Starting with values {:?}",
        entries.collect::<Vec<_>>().await
    );

    Ok(())
}
