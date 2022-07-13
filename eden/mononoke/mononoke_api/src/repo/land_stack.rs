/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks_movement::BookmarkKindRestrictions;
use bytes::Bytes;
use cloned::cloned;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use mononoke_types::ChangesetId;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::RangeNodeStream;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

pub use bookmarks_movement::PushrebaseOutcome;

impl RepoContext {
    /// Land a stack of commits to a bookmark via pushrebase.
    pub async fn land_stack(
        &self,
        bookmark: impl AsRef<str>,
        head: ChangesetId,
        base: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
        push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
    ) -> Result<PushrebaseOutcome, MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = self.skiplist_index().clone();

        // Check that base is an ancestor of the head commit, and fail with an
        // appropriate error message if that's not the case.
        if !lca_hint
            .is_ancestor(
                self.ctx(),
                &self.blob_repo().get_changeset_fetcher(),
                base,
                head,
            )
            .await?
        {
            return Err(MononokeError::InvalidRequest(format!(
                "Not a stack: base commit {} is not an ancestor of head commit {}",
                base, head,
            )));
        }

        // Find the commits we are interested in, and load their bonsai
        // changesets.   These are the commits that are ancestors of the head
        // commit and descendants of the base commit.
        let ctx = self.ctx();
        let blobstore = self.blob_repo().blobstore();
        let changesets: HashSet<_> = RangeNodeStream::new(
            ctx.clone(),
            self.blob_repo().get_changeset_fetcher(),
            base,
            head,
        )
        .compat()
        .map_err(MononokeError::from)
        .try_filter(|cs_id| future::ready(*cs_id != base))
        .map_ok(|cs_id| {
            cloned!(ctx);
            async move {
                cs_id
                    .load(&ctx, blobstore)
                    .map_err(MononokeError::from)
                    .await
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await?;

        // We CANNOT do remote pushrebase here otherwise it would result in an infinite
        // loop, as this code is used for remote pushrebase. Let's use local pushrebase.
        let outcome = LocalPushrebaseClient {
            ctx: self.ctx(),
            authz: self.authorization_context(),
            repo: self.inner_repo(),
            pushrebase_params: &self.config().pushrebase,
            lca_hint: &lca_hint,
            infinitepush_params: &self.config().infinitepush,
            hook_manager: self.hook_manager().as_ref(),
        }
        .pushrebase(
            &bookmark,
            changesets,
            pushvars,
            push_source,
            bookmark_restrictions,
        )
        .await?;

        Ok(outcome)
    }
}
