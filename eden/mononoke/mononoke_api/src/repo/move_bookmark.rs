/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Context;
use anyhow::format_err;
use bookmarks::AnnotatedTags;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks::MirrorBookmarkMove;
use bookmarks_movement::BookmarkInfoTransaction;
use bookmarks_movement::BookmarkUpdatePolicy;
use bookmarks_movement::BookmarkUpdateTargets;
use bookmarks_movement::CreateBookmarkOp;
use bookmarks_movement::UpdateBookmarkOp;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::sync_commit;
use hook_manager::manager::HookManagerRef;
use mononoke_types::ChangesetId;

use crate::MononokeRepo;
use crate::errors::MononokeError;
use crate::invalid_push_redirected_request;
use crate::repo::RepoContext;

impl<R: MononokeRepo> RepoContext<R> {
    /// Create operation for moving a bookmark
    async fn move_bookmark_op<'a>(
        &self,
        bookmark: &'_ BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&'a HashMap<String, Bytes>>,
        annotated_tags: Option<&'a AnnotatedTags>,
    ) -> Result<UpdateBookmarkOp<'a>, MononokeError> {
        self.start_write()?;

        // We need to find out where the bookmark currently points to in order
        // to move it.  Make sure to bypass any out-of-date caches.
        let old_target = match old_target {
            Some(old_target) => old_target,
            None => self
                .repo()
                .bookmarks()
                .get(
                    self.ctx().clone(),
                    bookmark,
                    bookmarks::Freshness::MostRecent,
                )
                .await
                .context("Failed to fetch old bookmark target")?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("bookmark '{bookmark}' does not exist"))
                })?,
        };

        fn make_move_op<'a>(
            bookmark: &'_ BookmarkKey,
            target: ChangesetId,
            old_target: ChangesetId,
            allow_non_fast_forward: bool,
            pushvars: Option<&'a HashMap<String, Bytes>>,
            annotated_tags: Option<&'a AnnotatedTags>,
        ) -> UpdateBookmarkOp<'a> {
            let op = UpdateBookmarkOp::new(
                bookmark.clone(),
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
            .with_pushvars(pushvars)
            .with_annotated_tags(annotated_tags);
            op.log_new_public_commits_to_scribe()
        }
        let push_redirector = self.push_redirector().await?;
        let op = if let Some(redirector) = push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(bookmark).await?;
            if &large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot move shared bookmark '{bookmark}' from small repo"
                )));
            }
            let ctx = self.ctx();
            let target = sync_commit(
                ctx,
                target,
                &redirector.small_to_large_commit_syncer,
                CandidateSelectionHint::Only,
                CommitSyncContext::PushRedirector,
                false,
            )
            .await?
            .ok_or_else(|| {
                format_err!(
                    "Error in move_bookmark absence of corresponding commit in target repo for {target}",
                )
            })?;
            let old_target = redirector
                .get_small_to_large_commit_equivalent(ctx, old_target)
                .await?;
            make_move_op(
                &large_bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
                annotated_tags,
            )
        } else {
            make_move_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
                annotated_tags,
            )
        };
        Ok(op)
    }

    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
        annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<(), MononokeError> {
        let update_op = self
            .move_bookmark_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
                annotated_tags,
            )
            .await?;
        let push_redirector = self.push_redirector().await?;
        if let Some(redirector) = push_redirector.as_ref() {
            let ctx = self.ctx();
            let log_id = update_op
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    &redirector.repo,
                    redirector.repo.hook_manager(),
                )
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.ensure_backsynced(ctx, log_id).await?;
        } else {
            update_op
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    self.repo(),
                    self.hook_manager().as_ref(),
                )
                .await?;
        }
        Ok(())
    }

    /// Move a bookmark with provided transaction
    pub async fn move_bookmark_with_transaction(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
        annotated_tags: Option<&AnnotatedTags>,
        txn: Option<Box<dyn BookmarkTransaction>>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Result<BookmarkInfoTransaction, MononokeError> {
        if self.push_redirector().await?.is_some() {
            return Err(invalid_push_redirected_request(
                "move_bookmark_with_transaction",
            ));
        }
        let update_op = self
            .move_bookmark_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
                annotated_tags,
            )
            .await?;
        let bookmark_info_transaction = update_op
            .run_with_transaction(
                self.ctx(),
                self.authorization_context(),
                self.repo(),
                self.hook_manager().as_ref(),
                txn,
                txn_hooks,
            )
            .await?;
        Ok(bookmark_info_transaction)
    }

    /// Mirror a contiguous chain of source bookmark moves to a `*_shadow`
    /// replica.
    ///
    /// modern_sync calls this to keep a replica's bookmark and
    /// bookmarks_update_log identical to the source, row for row. The whole
    /// chain applies as one compare-and-swap, and the store writes one
    /// bookmarks_update_log row per move, reusing each source move's log id,
    /// changesets, and reason. If the first move's `old` is `None`, the replica
    /// creates the bookmark instead of moving it.
    ///
    /// `moves` must be non-empty, ordered by strictly increasing `log_id`, and
    /// contiguous (each move's `old` equals the previous move's `new`). The
    /// replica must already hold every changeset the chain moves the bookmark
    /// to.
    pub async fn replay_identical_moves(
        &self,
        bookmark: &BookmarkKey,
        moves: Vec<MirrorBookmarkMove>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        // Replaying source log ids into a repo's bookmarks_update_log is only
        // ever correct for a mirror. Require the mirror upload permission on the
        // endpoint itself, so reaching it does not depend on which pushvars the
        // caller sent.
        self.authorization_context()
            .require_mirror_upload_operations(self.ctx(), self.repo())
            .await?;

        // The mirror endpoint targets `*_shadow` replicas, which are never push
        // redirected. Reject redirection rather than guess a mapping.
        if self.push_redirector().await?.is_some() {
            return Err(invalid_push_redirected_request("replay_identical_moves"));
        }

        // validate_chain enforces the chain is non-empty, ordered, and
        // contiguous. A first move whose `old` is `None` is a bookmark create;
        // anything else is an update. The constructors derive the targets from
        // the chain, so the targets and the moves cannot disagree.
        let starts_with_create = validate_chain(&moves)?.0.old.is_none();

        self.check_targets_are_known(&moves).await?;

        if starts_with_create {
            let op = CreateBookmarkOp::try_new_mirror(bookmark.clone(), moves)?
                .with_pushvars(pushvars)
                .log_new_public_commits_to_scribe();
            op.run(
                self.ctx(),
                self.authorization_context(),
                self.repo(),
                self.hook_manager().as_ref(),
            )
            .await?;
        } else {
            let op = UpdateBookmarkOp::try_new_mirror(bookmark.clone(), moves)?
                .with_pushvars(pushvars)
                .log_new_public_commits_to_scribe();
            op.run(
                self.ctx(),
                self.authorization_context(),
                self.repo(),
                self.hook_manager().as_ref(),
            )
            .await?;
        }
        Ok(())
    }

    /// Reject the chain unless the repo holds every changeset it moves the
    /// bookmark to. The caller names changesets by bonsai id, so nothing on the
    /// way in proves the repo received them, and a move to a changeset the repo
    /// does not have would leave the bookmark pointing at nothing.
    ///
    /// Only the `new` ids need a check. Each later move's `old` is the previous
    /// move's `new`, and the first move's `old` must match the bookmark's
    /// current value, which the compare-and-swap enforces. `validate_chain`
    /// caps the chain length, so this is one bounded commit graph query.
    async fn check_targets_are_known(
        &self,
        moves: &[MirrorBookmarkMove],
    ) -> Result<(), MononokeError> {
        let known: HashSet<ChangesetId> = self
            .repo()
            .commit_graph()
            .known_changesets(self.ctx(), moves.iter().map(|m| m.new).collect())
            .await?
            .into_iter()
            .collect();

        match moves.iter().find(|m| !known.contains(&m.new)) {
            Some(m) => Err(MononokeError::InvalidRequest(format!(
                "bookmark move at log id {} targets changeset {}, which the repo does not have",
                m.log_id, m.new
            ))),
            None => Ok(()),
        }
    }
}

/// Upper bound on the number of moves in one mirror chain. Every move costs a
/// row in a single multi-row `AddBookmarkLog` INSERT run inside the
/// per-bookmark bookmark transaction, so an unbounded chain could build an
/// oversized statement and hold the per-bookmark lock for a long time. The cap
/// also bounds the commit graph query that checks the chain's targets.
/// modern_sync sends chains far shorter than this; the cap only rejects a
/// pathological or buggy caller.
const MAX_MIRROR_CHAIN_LEN: usize = 10_000;

/// Check that the moves form a valid chain and return the first and last move.
///
/// A valid chain is non-empty, no longer than [`MAX_MIRROR_CHAIN_LEN`], ordered
/// by strictly increasing `log_id`, and contiguous: each move's `old` equals the
/// previous move's `new`. Only the first move may have `old` set to `None`,
/// which marks a bookmark create.
fn validate_chain(
    moves: &[MirrorBookmarkMove],
) -> Result<(&MirrorBookmarkMove, &MirrorBookmarkMove), MononokeError> {
    if moves.len() > MAX_MIRROR_CHAIN_LEN {
        return Err(MononokeError::InvalidRequest(format!(
            "bookmark move chain length {} exceeds the maximum of {}",
            moves.len(),
            MAX_MIRROR_CHAIN_LEN
        )));
    }

    let (first, rest) = moves
        .split_first()
        .ok_or_else(|| MononokeError::InvalidRequest("bookmark move chain is empty".to_string()))?;

    let mut prev = first;
    for next in rest {
        if next.log_id <= prev.log_id {
            return Err(MononokeError::InvalidRequest(format!(
                "bookmark move chain log ids are not strictly increasing: {} then {}",
                prev.log_id, next.log_id
            )));
        }
        if next.old != Some(prev.new) {
            return Err(MononokeError::InvalidRequest(format!(
                "bookmark move chain is not contiguous at log id {}",
                next.log_id
            )));
        }
        prev = next;
    }

    Ok((first, prev))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Result;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::repo::Repo;

    fn csid(byte: u8) -> ChangesetId {
        ChangesetId::from_str(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    /// A move that follows on from `old` to `new`.
    fn mv(log_id: u64, old: Option<u8>, new: u8) -> MirrorBookmarkMove {
        MirrorBookmarkMove {
            log_id,
            old: old.map(csid),
            new: csid(new),
            reason: BookmarkUpdateReason::ApiRequest,
        }
    }

    fn error_message(moves: &[MirrorBookmarkMove]) -> String {
        match validate_chain(moves) {
            Err(MononokeError::InvalidRequest(msg)) => msg,
            Err(other) => panic!("expected InvalidRequest, got {other:?}"),
            Ok(_) => panic!("expected the chain to be rejected"),
        }
    }

    #[mononoke::test]
    fn validate_chain_accepts_an_update_chain() {
        let moves = vec![mv(1, Some(1), 2), mv(2, Some(2), 3), mv(3, Some(3), 4)];
        let (first, last) = validate_chain(&moves).expect("chain is valid");
        assert_eq!(first, &moves[0]);
        assert_eq!(last, &moves[2]);
        // An update chain must not dispatch to CreateBookmarkOp.
        assert!(first.old.is_some());
    }

    #[mononoke::test]
    fn validate_chain_accepts_a_create_chain() {
        let moves = vec![mv(1, None, 1), mv(2, Some(1), 2)];
        let (first, last) = validate_chain(&moves).expect("chain is valid");
        assert_eq!(last, &moves[1]);
        // Only a `None` first `old` dispatches to CreateBookmarkOp.
        assert!(first.old.is_none());
    }

    #[mononoke::test]
    fn validate_chain_accepts_a_single_move() {
        let moves = vec![mv(7, Some(1), 2)];
        let (first, last) = validate_chain(&moves).expect("chain is valid");
        assert_eq!(first, last);
    }

    #[mononoke::test]
    fn validate_chain_rejects_an_empty_chain() {
        assert!(error_message(&[]).contains("empty"));
    }

    #[mononoke::test]
    fn validate_chain_rejects_a_chain_over_the_maximum_length() {
        let moves: Vec<_> = (0..=MAX_MIRROR_CHAIN_LEN)
            .map(|i| mv(i as u64 + 1, Some(1), 2))
            .collect();
        assert!(error_message(&moves).contains("exceeds the maximum"));
    }

    #[mononoke::test]
    fn validate_chain_rejects_repeated_log_ids() {
        let moves = vec![mv(1, Some(1), 2), mv(1, Some(2), 3)];
        assert!(error_message(&moves).contains("not strictly increasing"));
    }

    #[mononoke::test]
    fn validate_chain_rejects_decreasing_log_ids() {
        let moves = vec![mv(2, Some(1), 2), mv(1, Some(2), 3)];
        assert!(error_message(&moves).contains("not strictly increasing"));
    }

    #[mononoke::test]
    fn validate_chain_rejects_a_gap_between_moves() {
        // The second move starts from a changeset the first did not leave the
        // bookmark on, so one compare-and-swap would skip a move.
        let moves = vec![mv(1, Some(1), 2), mv(2, Some(3), 4)];
        assert!(error_message(&moves).contains("not contiguous"));
    }

    #[mononoke::test]
    fn validate_chain_rejects_a_create_after_the_first_move() {
        // Only the first move may omit `old`.
        let moves = vec![mv(1, Some(1), 2), mv(2, None, 3)];
        assert!(error_message(&moves).contains("not contiguous"));
    }

    /// A repo holding two commits, returned with their changeset ids.
    async fn repo_with_two_commits(
        ctx: &CoreContext,
    ) -> Result<(RepoContext<Repo>, ChangesetId, ChangesetId)> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let first = CreateCommitContext::new_root(ctx, &repo)
            .add_file("a", "1")
            .commit()
            .await?;
        let second = CreateCommitContext::new(ctx, &repo, vec![first])
            .add_file("b", "2")
            .commit()
            .await?;
        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        Ok((repo_ctx, first, second))
    }

    fn move_to(log_id: u64, old: Option<ChangesetId>, new: ChangesetId) -> MirrorBookmarkMove {
        MirrorBookmarkMove {
            log_id,
            old,
            new,
            reason: BookmarkUpdateReason::ApiRequest,
        }
    }

    #[mononoke::fbinit_test]
    async fn check_targets_are_known_accepts_changesets_the_repo_has(
        fb: FacebookInit,
    ) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (repo_ctx, first, second) = repo_with_two_commits(&ctx).await?;

        let moves = vec![move_to(1, None, first), move_to(2, Some(first), second)];

        repo_ctx.check_targets_are_known(&moves).await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn check_targets_are_known_rejects_a_changeset_the_repo_lacks(
        fb: FacebookInit,
    ) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (repo_ctx, first, _second) = repo_with_two_commits(&ctx).await?;

        // The second move targets a changeset that was never uploaded, so the
        // bookmark would end up pointing at nothing.
        let missing = csid(0xab);
        let moves = vec![move_to(1, None, first), move_to(2, Some(first), missing)];

        let msg = match repo_ctx.check_targets_are_known(&moves).await {
            Err(MononokeError::InvalidRequest(msg)) => msg,
            Err(other) => panic!("expected InvalidRequest, got {other:?}"),
            Ok(()) => panic!("expected the chain to be rejected"),
        };
        assert!(msg.contains("log id 2"), "unexpected message: {msg}");
        assert!(
            msg.contains(&missing.to_string()),
            "unexpected message: {msg}"
        );
        Ok(())
    }
}
