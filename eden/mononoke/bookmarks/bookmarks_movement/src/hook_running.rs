/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream::{self, StreamExt, TryStreamExt};
use futures_stats::TimedFutureExt;
use hooks::{HookManager, HookOutcome};
use metaconfig_types::BookmarkAttrs;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use scuba_ext::ScubaSampleBuilderExt;
use tunables::tunables;

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

/// Load bonsais not already in `new_changesets` that are ancestors of `head`
/// but not ancestors of `base` or any of the `hooks_skip_ancestors_of`
/// bookmarks for the named bookmark.
///
/// These are the additional bonsais that we need to run hooks on for bookmark
/// moves.
pub async fn load_additional_bonsais(
    ctx: &CoreContext,
    repo: &BlobRepo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    bookmark_attrs: &BookmarkAttrs,
    bookmark: &BookmarkName,
    head: ChangesetId,
    base: Option<ChangesetId>,
    new_changesets: &HashMap<ChangesetId, BonsaiChangeset>,
) -> Result<HashSet<BonsaiChangeset>> {
    let mut exclude_bookmarks: HashSet<_> = bookmark_attrs
        .select(bookmark)
        .map(|params| params.hooks_skip_ancestors_of.iter())
        .flatten()
        .cloned()
        .collect();
    exclude_bookmarks.remove(bookmark);

    let mut excludes: HashSet<_> = stream::iter(exclude_bookmarks)
        .map(|bookmark| repo.bookmarks().get(ctx.clone(), &bookmark))
        .buffered(100)
        .try_filter_map(|maybe_cs_id| async move { Ok(maybe_cs_id) })
        .try_collect()
        .await?;
    excludes.extend(base);

    let range = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &repo.get_changeset_fetcher(),
        lca_hint.clone(),
        vec![head],
        excludes.into_iter().collect(),
    )
    .compat()
    .try_filter(|bcs_id| {
        let exists = new_changesets.contains_key(bcs_id);
        future::ready(!exists)
    });

    let limit = match tunables().get_hooks_additional_changesets_limit() {
        limit if limit > 0 => limit as usize,
        _ => std::usize::MAX,
    };

    if tunables().get_run_hooks_on_additional_changesets() {
        let bonsais = range
            .and_then({
                let mut count = 0;
                move |bcs_id| {
                    count += 1;
                    if count > limit {
                        future::ready(Err(anyhow!(
                            "hooks additional changesets limit reached at {}",
                            bcs_id
                        )))
                    } else {
                        future::ready(Ok(bcs_id))
                    }
                }
            })
            .map(|res| async move {
                match res {
                    Ok(bcs_id) => Ok(bcs_id.load(ctx.clone(), repo.blobstore()).await?),
                    Err(e) => Err(e),
                }
            })
            .buffered(100)
            .try_collect::<HashSet<_>>()
            .await?;

        ctx.scuba()
            .clone()
            .add("hook_running_additional_changesets", bonsais.len())
            .log_with_msg("Running hooks for additional changesets", None);
        Ok(bonsais)
    } else {
        // Logging-only mode.  Work out how many changesets we would have run
        // on, and whether the limit would have been reached.
        let count = range
            .take(limit)
            .try_fold(0usize, |acc, _| async move { Ok(acc + 1) })
            .await?;

        let mut scuba = ctx.scuba().clone();
        scuba.add("hook_running_additional_changesets", count);
        if count >= limit {
            scuba.add("hook_running_additional_changesets_limit_reached", true);
        }
        scuba.log_with_msg("Hook running skipping additional changesets", None);
        Ok(HashSet::new())
    }
}
