/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{Context, Result};
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use hooks::{CrossRepoPushSource, HookManager, HookOutcome};
use mononoke_types::BonsaiChangeset;
use scuba_ext::ScubaSampleBuilderExt;
use tunables::tunables;

use crate::BookmarkMovementError;

pub async fn run_hooks(
    ctx: &CoreContext,
    hook_manager: &HookManager,
    bookmark: &BookmarkName,
    changesets: impl Iterator<Item = &BonsaiChangeset> + Clone,
    pushvars: Option<&HashMap<String, Bytes>>,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<(), BookmarkMovementError> {
    if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
        if tunables().get_disable_running_hooks_in_pushredirected_repo() {
            let cs_ids: Vec<_> = changesets
                .map(|cs| cs.get_changeset_id().to_string())
                .take(10) // Limit how many commit we are going to log
                .collect();
            ctx.scuba()
                .clone()
                .add("bookmark", bookmark.to_string())
                .add("changesets", cs_ids)
                .log_with_msg("Hook execution in pushredirected repo was disabled", None);
            return Ok(());
        }
    }

    let (stats, outcomes) = hook_manager
        .run_hooks_for_bookmark(&ctx, changesets, bookmark, pushvars, cross_repo_push_source)
        .timed()
        .await;
    let outcomes = outcomes.with_context(|| format!("Failed to run hooks for {}", bookmark))?;

    let rejections: Vec<_> = outcomes
        .into_iter()
        .filter_map(HookOutcome::into_rejection)
        .collect();

    ctx.scuba()
        .clone()
        .add_future_stats(&stats)
        .add("hook_rejections", rejections.len())
        .log_with_msg("Executed hooks", None);

    if rejections.is_empty() {
        Ok(())
    } else {
        Err(BookmarkMovementError::HookFailure(rejections))
    }
}
