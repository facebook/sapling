/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Prepare a manifest re-pin for a branch: build the generated manifest commit
//! and pre-derive its git identity. The bookmark is moved by the caller's
//! transaction, not here.

use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use filestore::FilestoreConfigRef;
use git_types::MappedGitCommitId;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

use crate::manifest_commit::create_manifest_commit;

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
