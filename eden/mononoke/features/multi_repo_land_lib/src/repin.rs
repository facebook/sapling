/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Manifest re-pin: build the generated manifest commit for a branch and, in
//! `repin_manifest_branch`, atomically move the bookmark to it under a CAS check.

use std::time::Duration;
use std::time::Instant;

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
use changesets_creation::save_changesets;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use dbbookmarks::store::SqlBookmarksRef;
use derivation_queue_thrift::DerivationPriority;
use filestore::FilestoreConfigRef;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use git_types::MappedGitCommitId;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use multi_repo_bookmarks_transaction::MultiRepoBookmarksTransaction;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;

use crate::manifest_commit::build_manifest_commit;
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

/// Where the CAS baseline — the head the bookmark move will be compared
/// against — comes from.
#[derive(Debug, Clone, Copy)]
pub enum CasBaseline {
    /// Read the branch head now and baseline against that. Only for content
    /// that was NOT generated from an earlier read: re-reading silently adopts
    /// anything that landed in between, so the CAS then succeeds and
    /// overwrites that land.
    CurrentHead,
    /// The head the content was generated from. A branch that moved since
    /// then fails the CAS instead of being overwritten.
    GeneratedFrom(ChangesetId),
}

/// The inputs for one branch's generated manifest commit.
pub struct ManifestCommitSpec<'a> {
    pub bookmark: &'a BookmarkKey,
    pub manifest_path: &'a NonRootMPath,
    pub content: Bytes,
    /// Recorded as the author and committer of the generated commit.
    pub service_identity: &'a str,
    /// Commit message. `None` falls back to a bare "updated the manifest",
    /// which says nothing about what the re-pin changed.
    pub message: Option<String>,
    /// Parent for the generated commit when the caller's own manifest edit
    /// must sit between the head and the generated commit; the baseline head
    /// otherwise. Never the CAS baseline — that stays the branch head.
    pub parent_override: Option<ChangesetId>,
    pub baseline: CasBaseline,
}

/// Build the manifest commit described by `spec` and pre-derive its git
/// identity.
///
/// Derivation runs BEFORE the caller's transaction so a derivation failure never
/// leaves a bookmark moved.
pub async fn prepare_manifest_commit<R>(
    ctx: &CoreContext,
    repo: &R,
    spec: ManifestCommitSpec<'_>,
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
    prepare_manifest_commits(ctx, repo, vec![spec], 1)
        .await?
        .pop()
        .ok_or_else(|| anyhow!("prepare_manifest_commits returned nothing for one spec"))
}

struct OwnedSpec {
    bookmark: BookmarkKey,
    manifest_path: NonRootMPath,
    content: Bytes,
    service_identity: String,
    message: Option<String>,
    parent_override: Option<ChangesetId>,
    baseline: CasBaseline,
}

/// Generated commits per `save_changesets` call.
const SAVE_CHUNK: usize = 100;

/// Wall time of each `prepare_manifest_commits` phase and the spread of the
/// per-commit git-id derivations, for the caller's telemetry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrepareTimings {
    /// Head reads and commit construction.
    pub build: Duration,
    /// The `save_changesets` calls.
    pub save: Duration,
    /// The whole git-id derivation phase.
    pub derive: Duration,
    /// Median single derivation; zero when nothing was derived.
    pub derive_p50: Duration,
    /// Slowest single derivation; zero when nothing was derived.
    pub derive_max: Duration,
    pub commits: usize,
}

impl PrepareTimings {
    /// Median and maximum of a derivation latency sample.
    fn derive_spread(mut samples: Vec<Duration>) -> (Duration, Duration) {
        if samples.is_empty() {
            return (Duration::ZERO, Duration::ZERO);
        }
        samples.sort_unstable();
        (samples[samples.len() / 2], samples[samples.len() - 1])
    }
}

/// [`prepare_manifest_commit`] for many branches at once: content and commits
/// are built `concurrency` at a time, every commit is saved in ONE
/// `save_changesets` (one commit-graph write instead of one per branch), then
/// git identities are derived `concurrency` at a time. Results are in `specs`
/// order.
pub async fn prepare_manifest_commits<R>(
    ctx: &CoreContext,
    repo: &R,
    specs: Vec<ManifestCommitSpec<'_>>,
    concurrency: usize,
) -> Result<Vec<PreparedManifestCommit>>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef,
{
    Ok(
        prepare_manifest_commits_timed(ctx, repo, specs, concurrency)
            .await?
            .0,
    )
}

/// [`prepare_manifest_commits`] plus how long each phase took.
pub async fn prepare_manifest_commits_timed<R>(
    ctx: &CoreContext,
    repo: &R,
    specs: Vec<ManifestCommitSpec<'_>>,
    concurrency: usize,
) -> Result<(Vec<PreparedManifestCommit>, PrepareTimings)>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef,
{
    let concurrency = concurrency.max(1);
    let mut timings = PrepareTimings {
        commits: specs.len(),
        ..PrepareTimings::default()
    };
    // Owned copies: a borrowed spec inside the buffered futures makes the
    // closure higher-ranked over its lifetime, which the Thrift service's
    // `Send` bound rejects.
    let specs: Vec<OwnedSpec> = specs
        .into_iter()
        .map(|spec| OwnedSpec {
            bookmark: spec.bookmark.clone(),
            manifest_path: spec.manifest_path.clone(),
            content: spec.content,
            service_identity: spec.service_identity.to_string(),
            message: spec.message,
            parent_override: spec.parent_override,
            baseline: spec.baseline,
        })
        .collect();
    let started = Instant::now();
    let built: Vec<(ChangesetId, BonsaiChangeset)> = stream::iter(specs)
        .map(|spec| async move {
            let old_cs = match spec.baseline {
                CasBaseline::GeneratedFrom(head) => head,
                CasBaseline::CurrentHead => repo
                    .bookmarks()
                    .get(ctx.clone(), &spec.bookmark, Freshness::MostRecent)
                    .await?
                    .ok_or_else(|| {
                        anyhow!(
                            "manifest bookmark not found: {} in repo {}",
                            spec.bookmark,
                            repo.repo_identity().name()
                        )
                    })?,
            };
            let parent = spec.parent_override.unwrap_or(old_cs);
            let bcs = build_manifest_commit(
                ctx,
                repo,
                parent,
                &spec.manifest_path,
                spec.content,
                &spec.service_identity,
                spec.message,
            )
            .await?;
            anyhow::Ok((old_cs, bcs))
        })
        .buffered(concurrency)
        .try_collect()
        .await?;
    timings.build = started.elapsed();

    // `save_changesets` writes every bonsai's blob at once; chunking keeps that
    // burst bounded while still writing the graph a chunk at a time.
    let started = Instant::now();
    for chunk in built.chunks(SAVE_CHUNK) {
        save_changesets(
            ctx,
            repo,
            chunk.iter().map(|(_, bcs)| bcs.clone()).collect(),
        )
        .await?;
    }
    timings.save = started.elapsed();

    let started = Instant::now();
    let (prepared, samples): (Vec<PreparedManifestCommit>, Vec<Duration>) = stream::iter(built)
        .map(|(old_cs, bcs)| async move {
            let new_cs = bcs.get_changeset_id();
            let derive_started = Instant::now();
            let mapped_git = repo
                .repo_derived_data()
                .derive::<MappedGitCommitId>(ctx, new_cs, DerivationPriority::LOW)
                .await?;
            anyhow::Ok((
                PreparedManifestCommit {
                    old_cs,
                    new_cs,
                    mapped_git,
                },
                derive_started.elapsed(),
            ))
        })
        .buffered(concurrency)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .unzip();
    timings.derive = started.elapsed();
    (timings.derive_p50, timings.derive_max) = PrepareTimings::derive_spread(samples);
    Ok((prepared, timings))
}

/// Options for a single-branch re-pin.
#[derive(Debug, Clone)]
pub struct RepinOptions {
    /// Emit fire-and-forget scribe logging for the bookmark move.
    pub log_scribe: bool,
    /// See [`CasBaseline`]. `GeneratedFrom` whenever the caller read the
    /// branch itself and generated the content from that read.
    pub baseline: CasBaseline,
    /// See [`ManifestCommitSpec::message`].
    pub message: Option<String>,
}

impl Default for RepinOptions {
    fn default() -> Self {
        Self {
            log_scribe: true,
            baseline: CasBaseline::CurrentHead,
            message: None,
        }
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
    let PreparedManifestCommit {
        old_cs,
        new_cs,
        mapped_git,
    } = prepare_manifest_commit(
        ctx,
        repo,
        ManifestCommitSpec {
            bookmark,
            manifest_path,
            content: new_content,
            service_identity,
            message: opts.message.clone(),
            // No user-manifest parent: build on the head.
            parent_override: None,
            baseline: opts.baseline,
        },
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
