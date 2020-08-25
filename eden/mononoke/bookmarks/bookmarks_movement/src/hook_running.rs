/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use hooks::{HookManager, HookOutcome};
use mononoke_types::BonsaiChangeset;
use scuba_ext::ScubaSampleBuilderExt;

use crate::BookmarkMovementError;

pub async fn run_hooks(
    ctx: &CoreContext,
    hook_manager: &HookManager,
    bookmark: &BookmarkName,
    changesets: impl Iterator<Item = &BonsaiChangeset> + Clone,
    pushvars: Option<&HashMap<String, Bytes>>,
) -> Result<(), BookmarkMovementError> {
    let (stats, outcomes) = hook_manager
        .run_hooks_for_bookmark(&ctx, changesets, bookmark, pushvars)
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
