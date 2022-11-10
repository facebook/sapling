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
pub use bookmarks_movement::PushrebaseOutcome;
use bytes::Bytes;
use cloned::cloned;
use cross_repo_sync::types::Large;
use cross_repo_sync::types::Small;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use hooks::PushAuthoredBy;
use mononoke_types::ChangesetId;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::RangeNodeStream;
use unbundle::PushRedirector;

use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::Repo;

impl RepoContext {
    async fn convert_outcome(
        &self,
        redirector: PushRedirector<Repo>,
        outcome: Large<PushrebaseOutcome>,
    ) -> Result<Small<PushrebaseOutcome>, MononokeError> {
        let ctx = self.ctx();
        let PushrebaseOutcome {
            old_bookmark_value,
            head,
            retry_num,
            rebased_changesets,
            pushrebase_distance,
        } = outcome.0;
        redirector.backsync_latest(ctx).await?;
        Ok(Small(PushrebaseOutcome {
            old_bookmark_value: match old_bookmark_value {
                Some(val) => Some(
                    redirector
                        .get_large_to_small_commit_equivalent(ctx, val)
                        .await?,
                ),
                None => None,
            },
            head: redirector
                .get_large_to_small_commit_equivalent(ctx, head)
                .await?,
            retry_num,
            rebased_changesets: redirector
                .convert_pushrebased_changesets(ctx, rebased_changesets)
                .await?,
            pushrebase_distance,
        }))
    }

    /// Land a stack of commits to a bookmark via pushrebase.
    pub async fn land_stack(
        &self,
        bookmark: impl AsRef<str>,
        head: ChangesetId,
        base: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
        // TODO: Remove
        push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
        maybe_pushredirector: Option<(PushRedirector<Repo>, Large<RepoContext>)>,
        push_authored_by: PushAuthoredBy,
    ) -> Result<PushrebaseOutcome, MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = self.skiplist_index_arc();

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

        let outcome = if let Some((redirector, Large(large_repo))) = maybe_pushredirector {
            // run hooks on small repo
            bookmarks_movement::run_hooks(
                ctx,
                self.hook_manager().as_ref(),
                &bookmark,
                changesets.iter(),
                pushvars,
                CrossRepoPushSource::NativeToThisRepo,
                push_authored_by,
            )
            .await?;
            // Convert changesets to large repo
            let large_bookmark = redirector.small_to_large_bookmark(&bookmark).await?;
            let small_to_large = redirector
                .sync_uploaded_changesets(ctx, changesets, Some(&large_bookmark))
                .await?;
            // Land the mapped changesets on the large repo
            let outcome = LocalPushrebaseClient {
                ctx: large_repo.ctx(),
                authz: large_repo.authorization_context(),
                repo: large_repo.inner_repo(),
                lca_hint: &(large_repo.skiplist_index_arc() as Arc<dyn LeastCommonAncestorsHint>),
                hook_manager: large_repo.hook_manager().as_ref(),
            }
            .pushrebase(
                &large_bookmark,
                small_to_large.into_values().collect(),
                pushvars,
                CrossRepoPushSource::PushRedirected,
                bookmark_restrictions,
            )
            .await?;
            // Convert response back, finishing the land on the small repo
            self.convert_outcome(redirector, Large(outcome)).await?.0
        } else {
            LocalPushrebaseClient {
                ctx: self.ctx(),
                authz: self.authorization_context(),
                repo: self.inner_repo(),
                lca_hint: &lca_hint,
                hook_manager: self.hook_manager().as_ref(),
            }
            .pushrebase(
                &bookmark,
                changesets,
                pushvars,
                push_source,
                bookmark_restrictions,
            )
            .await?
        };

        Ok(outcome)
    }
}
