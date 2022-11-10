/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks_movement::BookmarkUpdatePolicy;
use bookmarks_movement::BookmarkUpdateTargets;
use bookmarks_movement::UpdateBookmarkOp;
use bytes::Bytes;
use hooks::HookManagerRef;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use skiplist::SkiplistIndexArc;
use tunables::tunables;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

impl RepoContext {
    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        // We need to find out where the bookmark currently points to in order
        // to move it.  Make sure to bypass any out-of-date caches.
        let old_target = match old_target {
            Some(old_target) => old_target,
            None => self
                .blob_repo()
                .bookmarks()
                .get(self.ctx().clone(), &bookmark)
                .await
                .context("Failed to fetch old bookmark target")?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("bookmark '{}' does not exist", bookmark))
                })?,
        };

        fn make_move_op<'a>(
            bookmark: &'a BookmarkName,
            target: ChangesetId,
            old_target: ChangesetId,
            allow_non_fast_forward: bool,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> UpdateBookmarkOp<'a> {
            let mut op = UpdateBookmarkOp::new(
                bookmark,
                BookmarkUpdateTargets {
                    old: old_target,
                    new: target,
                },
                if allow_non_fast_forward {
                    BookmarkUpdatePolicy::AnyPermittedByConfig
                } else {
                    BookmarkUpdatePolicy::FastForwardOnly
                },
                BookmarkUpdateReason::ApiRequest,
            )
            .with_pushvars(pushvars);
            if !tunables().get_disable_commit_scribe_logging_scs() {
                op = op.log_new_public_commits_to_scribe();
            }
            op
        }
        if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(&bookmark).await?;
            if large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot move shared bookmark '{}' from small repo",
                    bookmark
                )));
            }
            let ctx = self.ctx();
            let (target, old_target) = futures::try_join!(
                redirector.get_small_to_large_commit_equivalent(ctx, target),
                redirector.get_small_to_large_commit_equivalent(ctx, old_target),
            )?;
            make_move_op(
                &large_bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
            .run(
                self.ctx(),
                self.authorization_context(),
                redirector.repo.inner_repo(),
                &(redirector.repo.skiplist_index_arc() as Arc<dyn LeastCommonAncestorsHint>),
                redirector.repo.hook_manager(),
            )
            .await?;
            // Wait for bookmark to catch up on small repo
            redirector.backsync_latest(ctx).await?;
        } else {
            make_move_op(
                &bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
            .run(
                self.ctx(),
                self.authorization_context(),
                self.inner_repo(),
                &(self.skiplist_index_arc() as Arc<dyn LeastCommonAncestorsHint>),
                self.hook_manager().as_ref(),
            )
            .await?;
        }

        Ok(())
    }
}
