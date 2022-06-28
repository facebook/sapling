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
use metaconfig_types::BookmarkAttrs;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use tunables::tunables;

use crate::errors::MononokeError;
use crate::permissions::WritePermissionsModel;
use crate::repo_write::RepoWriteContext;

impl RepoWriteContext {
    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        let bookmark = bookmark.as_ref();
        self.check_method_permitted("move_bookmark")?;

        let bookmark = BookmarkName::new(bookmark)?;
        let bookmark_attrs =
            BookmarkAttrs::new(self.ctx().fb, self.config().bookmarks.clone()).await?;

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

        if let WritePermissionsModel::ServiceIdentity(service_identity) = &self.permissions_model {
            op = op.for_service(service_identity, &self.config().source_control_service);
        }

        op.run(
            self.ctx(),
            self.inner_repo(),
            &lca_hint,
            &self.config().infinitepush,
            &self.config().pushrebase,
            &bookmark_attrs,
            self.hook_manager().as_ref(),
            self.readonly_fetcher(),
        )
        .await?;

        Ok(())
    }
}
