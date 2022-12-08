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
use cross_repo_sync::CommitSyncOutcome;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use hooks::HookManagerRef;
use hooks::PushAuthoredBy;
use mononoke_types::ChangesetId;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::RangeNodeStream;
use skiplist::SkiplistIndexArc;
use unbundle::PushRedirector;

use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::Repo;

impl RepoContext {
    async fn convert_old_bookmark_value(
        &self,
        redirector: &PushRedirector<Repo>,
        bookmark_value: Large<Option<ChangesetId>>,
    ) -> anyhow::Result<Small<Option<ChangesetId>>> {
        let large_cs_id = match bookmark_value {
            Large(Some(cs_id)) => cs_id,
            Large(None) => return Ok(Small(None)),
        };
        let syncer = &redirector.large_to_small_commit_syncer;
        match syncer
            .get_commit_sync_outcome(self.ctx(), large_cs_id)
            .await?
        {
            None => anyhow::bail!(
                "Unexpected absence of CommitSyncOutcome for {} in {:?}",
                large_cs_id,
                syncer
            ),
            // EquivalentWorkingCopyAncestor is fine because the bookmark commit in the
            // large repo might not have come from the small repo
            Some(CommitSyncOutcome::RewrittenAs(small_cs_id, _))
            | Some(CommitSyncOutcome::EquivalentWorkingCopyAncestor(small_cs_id, _)) => {
                Ok(Small(Some(small_cs_id)))
            }
            Some(outcome) => anyhow::bail!(
                "Unexpected CommitSyncOutcome for {} in {:?}: {:?}",
                large_cs_id,
                syncer,
                outcome
            ),
        }
    }
    async fn convert_outcome(
        &self,
        redirector: &PushRedirector<Repo>,
        outcome: Large<PushrebaseOutcome>,
    ) -> Result<Small<PushrebaseOutcome>, MononokeError> {
        let ctx = self.ctx();
        let Large(PushrebaseOutcome {
            old_bookmark_value,
            head,
            retry_num,
            rebased_changesets,
            pushrebase_distance,
        }) = outcome;
        redirector.backsync_latest(ctx).await?;

        // Convert all fields from large to small repo
        let (Small(old_bookmark_value), head, rebased_changesets) = futures::try_join!(
            self.convert_old_bookmark_value(redirector, Large(old_bookmark_value)),
            redirector.get_large_to_small_commit_equivalent(ctx, head),
            redirector.convert_pushrebased_changesets(ctx, rebased_changesets)
        )?;

        Ok(Small(PushrebaseOutcome {
            old_bookmark_value,
            head,
            retry_num,
            rebased_changesets,
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
        bookmark_restrictions: BookmarkKindRestrictions,
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

        let outcome = if let Some(redirector) = self.push_redirector.as_ref() {
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
                ctx: self.ctx(),
                authz: self.authorization_context(),
                repo: &redirector.repo.inner,
                lca_hint: &(redirector.repo.skiplist_index_arc()
                    as Arc<dyn LeastCommonAncestorsHint>),
                hook_manager: redirector.repo.hook_manager(),
            }
            .pushrebase(
                &large_bookmark,
                small_to_large.into_values().collect(),
                pushvars,
                CrossRepoPushSource::PushRedirected,
                bookmark_restrictions,
                true, // log_new_public_commits_to_scribe
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
                CrossRepoPushSource::NativeToThisRepo,
                bookmark_restrictions,
                true, // log_new_public_commits_to_scribe
            )
            .await?
        };

        Ok(outcome)
    }
}
