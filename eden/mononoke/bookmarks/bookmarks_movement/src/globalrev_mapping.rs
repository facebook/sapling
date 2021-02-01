/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::PushrebaseParams;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;

use crate::BookmarkMovementError;

/// Verify that a bookmark push is allowable with regard to Globalrevs.
pub(crate) async fn check_globalrevs_push(
    ctx: &CoreContext,
    repo: &BlobRepo,
    lca_hint: &dyn LeastCommonAncestorsHint,
    pushrebase_params: &PushrebaseParams,
    bookmark: &BookmarkName,
    target: ChangesetId,
) -> Result<(), BookmarkMovementError> {
    // NOTE: Obviously this is a little racy, but the bookmark could move after we check, so it
    // doesn't matter.

    let globalrevs_publishing_bookmark =
        match pushrebase_params.globalrevs_publishing_bookmark.as_ref() {
            Some(b) => b,
            None => return Ok(()),
        };

    let publishing_cs_id = repo
        .get_bonsai_bookmark(ctx.clone(), globalrevs_publishing_bookmark)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Globalrevs publishing bookmark '{}' does not exist!",
                globalrevs_publishing_bookmark
            )
        })?;

    let is_ancestor = target == publishing_cs_id
        || lca_hint
            .is_ancestor(ctx, &repo.get_changeset_fetcher(), target, publishing_cs_id)
            .await?;

    if is_ancestor {
        return Ok(());
    }

    let e = BookmarkMovementError::RequiresAncestorOf {
        bookmark: bookmark.clone(),
        descendant_bookmark: globalrevs_publishing_bookmark.clone(),
    };

    Err(e)
}
