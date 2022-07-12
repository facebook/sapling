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
use bytes::Bytes;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
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

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = self.skiplist_index().clone();

        // Move the bookmark.
        let mut op = bookmarks_movement::UpdateBookmarkOp::new(
            &bookmark,
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

        op.run(
            self.ctx(),
            self.authorization_context(),
            self.inner_repo(),
            &lca_hint,
            &self.config().infinitepush,
            &self.config().pushrebase,
            self.hook_manager().as_ref(),
        )
        .await?;

        Ok(())
    }
}
