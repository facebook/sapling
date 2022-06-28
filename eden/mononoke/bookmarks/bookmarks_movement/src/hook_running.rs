/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use hooks::HookOutcome;
use hooks::PushAuthoredBy;
use mononoke_types::BonsaiChangeset;
use tunables::tunables;

use crate::BookmarkMovementError;

pub async fn is_admin_bypass(
    ctx: &CoreContext,
    hook_manager: &HookManager,
    pushvars: Option<&HashMap<String, Bytes>>,
) -> Result<bool> {
    let pushvars = match pushvars {
        Some(pushvars) => pushvars,
        None => {
            return Ok(false);
        }
    };

    if !pushvars.contains_key("BYPASS_ALL_HOOKS") {
        return Ok(false);
    }

    if !hook_manager
        .get_admin_perm_checker()
        .is_member(ctx.metadata().identities())
        .await
        .with_context(|| "Error when checking BYPASS_ALL_HOOKS permissions")?
    {
        return Err(anyhow!(
            "In order to use BYPASS_ALL_HOOKS pushvar one needs to be member of the scm group."
        ));
    }

    Ok(true)
}

fn take_n_changeset_ids<'a>(
    changesets: impl Iterator<Item = &'a BonsaiChangeset> + Clone,
    n: usize,
) -> Vec<String> {
    changesets
        .map(|cs| cs.get_changeset_id().to_string())
        .take(n)
        .collect()
}

pub async fn run_hooks(
    ctx: &CoreContext,
    hook_manager: &HookManager,
    bookmark: &BookmarkName,
    changesets: impl Iterator<Item = &BonsaiChangeset> + Clone,
    pushvars: Option<&HashMap<String, Bytes>>,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<(), BookmarkMovementError> {
    if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
        if tunables().get_disable_running_hooks_in_pushredirected_repo() {
            let cs_ids = take_n_changeset_ids(changesets, 10);
            ctx.scuba()
                .clone()
                .add("bookmark", bookmark.to_string())
                .add("changesets", cs_ids)
                .log_with_msg("Hook execution in pushredirected repo was disabled", None);
            return Ok(());
        }
    }

    if is_admin_bypass(ctx, hook_manager, pushvars).await? || hook_manager.all_hooks_bypassed() {
        let mut scuba_bypassed_commits = hook_manager.scuba_bypassed_commits().clone();
        let cs_ids = take_n_changeset_ids(changesets, 10);

        scuba_bypassed_commits
            .add_metadata(ctx.metadata())
            .add("bookmark", bookmark.to_string())
            .add("changesets", cs_ids)
            .add("repo_name", hook_manager.repo_name().clone());

        if let Some(pushvars) = pushvars {
            scuba_bypassed_commits.add(
                "pushvars",
                pushvars
                    .iter()
                    .map(|(key, val)| format!("{}={:?}", key, val))
                    .collect::<Vec<_>>(),
            );
        }

        scuba_bypassed_commits
            .log_with_msg("Bypassed all hooks using BYPASS_ALL_HOOKS pushvar.", None);
        return Ok(());
    }

    let (stats, outcomes) = hook_manager
        .run_hooks_for_bookmark(
            ctx,
            changesets,
            bookmark,
            pushvars,
            cross_repo_push_source,
            push_authored_by,
        )
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
