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
use changeset_info::ChangesetInfo;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
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

    let entries_vec = entries.collect::<Vec<_>>().await;

    for entry in entries_vec {
        info!(app.logger(), "Entry {:?}", entry);
        let raw_entry = entry?;
        let from = raw_entry.from_changeset_id.map_or(vec![], |val| vec![val]);
        let to = raw_entry.to_changeset_id.map_or(vec![], |val| vec![val]);

        let mut res = repo
            .commit_graph
            .ancestors_difference_stream(&ctx, to, from)
            .await?;

        while let Some(cs_id) = res.try_next().await? {
            info!(app.logger(), "Found commit {:?}", cs_id);
            let cs_info = repo
                .repo_derived_data
                .derive::<ChangesetInfo>(&ctx, cs_id.clone())
                .await?;
            info!(app.logger(), "Commit info {:?}", cs_info);
        }
    }

    Ok(())
}
