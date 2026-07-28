/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Manifest re-pin: build the generated manifest commit for a branch and, in
//! `repin_manifest_branch`, atomically move the bookmark to it under a CAS check.

use anyhow::Result;
use anyhow::anyhow;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use dbbookmarks::store::SqlBookmarksRef;
use derivation_queue_thrift::DerivationPriority;
use filestore::FilestoreConfigRef;
use git_types::MappedGitCommitId;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use multi_repo_bookmarks_transaction::MultiRepoBookmarksTransaction;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;

use crate::manifest_commit::create_manifest_commit;
use crate::scribe::log_scribe_bookmark_update;

/// A manifest commit prepared for one branch, ready for the caller's transaction.
#[derive(Debug)]
pub struct PreparedManifestCommit {
    /// Live head at prepare time; the CAS baseline for the caller's `txn.update`.
    pub old_cs: ChangesetId,
    /// The generated manifest commit to move the bookmark to.
    pub new_cs: ChangesetId,
    /// Pre-derived git identity of `new_cs`.
    pub mapped_git: MappedGitCommitId,
}

/// Read the branch head, build the manifest commit on `parent_override` (or the
/// head when `None`), and pre-derive its git identity.
///
/// `old_cs` is always the live head (the CAS baseline), even when
/// `parent_override` differs. Derivation runs BEFORE the caller's transaction so
/// a derivation failure never leaves a bookmark moved.
pub async fn prepare_manifest_commit<R>(
    ctx: &CoreContext,
    repo: &R,
    bookmark: &BookmarkKey,
    parent_override: Option<ChangesetId>,
    manifest_path: &NonRootMPath,
    new_content: Bytes,
    service_identity: &str,
) -> Result<PreparedManifestCommit>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef,
{
    let old_cs = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, Freshness::MostRecent)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "manifest bookmark not found: {bookmark} in repo {}",
                repo.repo_identity().name()
            )
        })?;

    let parent = parent_override.unwrap_or(old_cs);

    let new_cs = create_manifest_commit(
        ctx,
        repo,
        parent,
        manifest_path,
        new_content,
        service_identity,
    )
    .await?;

    let mapped_git = repo
        .repo_derived_data()
        .derive::<MappedGitCommitId>(ctx, new_cs, DerivationPriority::LOW)
        .await?;

    Ok(PreparedManifestCommit {
        old_cs,
        new_cs,
        mapped_git,
    })
}

/// Options for a single-branch re-pin.
#[derive(Debug, Clone)]
pub struct RepinOptions {
    /// Emit fire-and-forget scribe logging for the bookmark move.
    pub log_scribe: bool,
}

impl Default for RepinOptions {
    fn default() -> Self {
        Self { log_scribe: true }
    }
}

/// Outcome of a single-branch re-pin.
#[derive(Debug)]
pub enum RepinOutcome {
    Moved {
        new_cs: ChangesetId,
        mapped_git: MappedGitCommitId,
    },
    /// CAS failed (head moved concurrently); nothing changed, not rebased.
    CasFailure,
}

/// Generate the manifest commit on the current head and atomically move the
/// bookmark to it under a CAS check. Re-pins exactly ONE branch so callers can
/// isolate per-branch errors.
///
/// A CAS failure returns `CasFailure` without retrying or rebasing; scribe
/// logging (when `opts.log_scribe`) is fire-and-forget and never flips the
/// outcome.
pub async fn repin_manifest_branch<R>(
    ctx: &CoreContext,
    repo: &R,
    bookmark: &BookmarkKey,
    manifest_path: &NonRootMPath,
    new_content: Bytes,
    service_identity: &str,
    opts: &RepinOptions,
) -> Result<RepinOutcome>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef
        + SqlBookmarksRef
        + RepoConfigRef
        + BonsaiGitMappingRef
        + BonsaiGlobalrevMappingRef
        + PhasesRef
        + Sync,
{
    // No user-manifest parent: build on the head.
    let PreparedManifestCommit {
        old_cs,
        new_cs,
        mapped_git,
    } = prepare_manifest_commit(
        ctx,
        repo,
        bookmark,
        None,
        manifest_path,
        new_content,
        service_identity,
    )
    .await?;

    let mut txn = MultiRepoBookmarksTransaction::new(
        ctx.clone(),
        repo.sql_bookmarks().write_connection().clone(),
    );
    txn.update(
        repo.repo_identity().id(),
        bookmark,
        new_cs,
        old_cs,
        BookmarkUpdateReason::MultiRepoLand,
    )?;

    if !txn.commit().await?.is_success() {
        return Ok(RepinOutcome::CasFailure);
    }

    if opts.log_scribe {
        let info = BookmarkInfo {
            bookmark_name: bookmark.clone(),
            bookmark_kind: BookmarkKind::Publishing,
            operation: BookmarkOperation::Update(old_cs, new_cs),
            reason: BookmarkUpdateReason::MultiRepoLand,
        };
        log_scribe_bookmark_update(ctx, repo, &info, Some(new_cs)).await;
    }

    Ok(RepinOutcome::Moved { new_cs, mapped_git })
}
