/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use bookmarks_movement::{BookmarkUpdatePolicy, BookmarkUpdateTargets};
use metaconfig_types::BookmarkAttrs;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;

use crate::errors::MononokeError;
use crate::repo_write::RepoWriteContext;

impl RepoWriteContext {
    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        target: ChangesetId,
        allow_non_fast_forward: bool,
    ) -> Result<(), MononokeError> {
        let bookmark = bookmark.as_ref();
        self.check_method_permitted("move_bookmark")?;
        self.check_bookmark_modification_permitted(bookmark)?;

        let bookmark = BookmarkName::new(bookmark)?;
        let bookmark_attrs = BookmarkAttrs::new(self.config().bookmarks.clone());

        // We need to find out where the bookmark currently points to in order
        // to move it.  Make sure to bypass any out-of-date caches.
        let old_target = self
            .blob_repo()
            .bookmarks()
            .get(self.ctx().clone(), &bookmark)
            .await
            .context("Failed to fetch old bookmark target")?
            .ok_or_else(|| {
                MononokeError::InvalidRequest(format!("bookmark '{}' does not exist", bookmark))
            })?;

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = self.skiplist_index().clone();

        // Move the bookmark.
        bookmarks_movement::UpdateBookmarkOp::new(
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
        .run(
            self.ctx(),
            self.blob_repo(),
            &lca_hint,
            &self.config().infinitepush,
            &self.config().pushrebase,
            &bookmark_attrs,
            self.hook_manager().as_ref(),
        )
        .await?;

        Ok(())
    }
}
