/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Manifest re-pin: build the generated manifest commit for a branch and, in
//! `repin_manifest_branch`, atomically move the bookmark to it under a CAS check.

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
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
#[derive(Debug, Clone)]
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
        + RepoDerivedDataRef
        + BonsaiGitMappingRef,
{
    prepare_manifest_commits(ctx, repo, vec![spec], 1, GitIdDerivation::PerCommit)
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

/// Upper bound on generated commits per local batch derivation call; a safety
/// valve on one call's bonsai loads, not a tuning knob. A land fits in one.
const LOCAL_DERIVE_BATCH_MAX: usize = 500;

/// How `prepare_manifest_commits` obtains each generated commit's git identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitIdDerivation {
    /// One derivation per commit through the repo's derivation path.
    PerCommit,
    /// One concurrent batch over the siblings whose parents already have git
    /// ids; the rest, and the whole batch if it fails, go `PerCommit`.
    LocalBatch,
}

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
    /// Median per-commit derivation; a batch call's time is split evenly over
    /// its commits. Zero when nothing was derived.
    pub derive_p50: Duration,
    /// Slowest per-commit derivation, split the same way; zero when nothing
    /// was derived.
    pub derive_max: Duration,
    pub commits: usize,
    /// Commits a parent without a git id kept out of the batches.
    pub derive_unbatchable_commits: usize,
    /// Commits whose batch failed and were re-derived one at a time.
    pub derive_fallback_commits: usize,
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
/// git identities are derived per `git_ids`: `concurrency` at a time, or in
/// one local batch. Results are in `specs` order.
pub async fn prepare_manifest_commits<R>(
    ctx: &CoreContext,
    repo: &R,
    specs: Vec<ManifestCommitSpec<'_>>,
    concurrency: usize,
    git_ids: GitIdDerivation,
) -> Result<Vec<PreparedManifestCommit>>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef
        + BonsaiGitMappingRef,
{
    Ok(
        prepare_manifest_commits_timed(ctx, repo, specs, concurrency, git_ids)
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
    git_ids: GitIdDerivation,
) -> Result<(Vec<PreparedManifestCommit>, PrepareTimings)>
where
    R: RepoIdentityRef
        + BookmarksRef
        + RepoBlobstoreRef
        + FilestoreConfigRef
        + CommitGraphRef
        + CommitGraphWriterRef
        + RepoDerivedDataRef
        + BonsaiGitMappingRef,
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
    let derived = match git_ids {
        GitIdDerivation::PerCommit => {
            let (prepared, samples) = derive_per_commit(ctx, repo, built, concurrency).await?;
            Derived {
                prepared,
                samples,
                unbatchable_commits: 0,
                fallback_commits: 0,
            }
        }
        GitIdDerivation::LocalBatch => derive_local_batches(ctx, repo, built, concurrency).await?,
    };
    timings.derive = started.elapsed();
    timings.derive_unbatchable_commits = derived.unbatchable_commits;
    timings.derive_fallback_commits = derived.fallback_commits;
    (timings.derive_p50, timings.derive_max) = PrepareTimings::derive_spread(derived.samples);
    Ok((derived.prepared, timings))
}

/// The derive phase's output: prepared commits in input order and one
/// latency sample per commit.
struct Derived {
    prepared: Vec<PreparedManifestCommit>,
    samples: Vec<Duration>,
    unbatchable_commits: usize,
    fallback_commits: usize,
}

/// One derivation per commit; returns the commits in input order with each
/// commit's derivation latency.
async fn derive_per_commit<R>(
    ctx: &CoreContext,
    repo: &R,
    built: Vec<(ChangesetId, BonsaiChangeset)>,
    concurrency: usize,
) -> Result<(Vec<PreparedManifestCommit>, Vec<Duration>)>
where
    R: RepoDerivedDataRef,
{
    stream::iter(built)
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
        .await
        .map(|prepared| prepared.into_iter().unzip())
}

/// The independent siblings are derived here in one concurrent batch: one
/// bonsai load, one blob flush and one mapping write for the land instead of
/// per commit. One mapping read over every distinct parent first routes
/// commits whose parent has no git id (the batch precondition) to the
/// per-commit path, and a batch that fails is derived one commit at a time
/// instead, so a land never fails for the batch path alone.
async fn derive_local_batches<R>(
    ctx: &CoreContext,
    repo: &R,
    built: Vec<(ChangesetId, BonsaiChangeset)>,
    concurrency: usize,
) -> Result<Derived>
where
    R: RepoDerivedDataRef + BonsaiGitMappingRef,
{
    let order: Vec<(ChangesetId, ChangesetId)> = built
        .iter()
        .map(|(old_cs, bcs)| (*old_cs, bcs.get_changeset_id()))
        .collect();

    let parents: Vec<ChangesetId> = built
        .iter()
        .flat_map(|(_, bcs)| bcs.parents())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mapped_parents: HashSet<ChangesetId> = if parents.is_empty() {
        HashSet::new()
    } else {
        repo.bonsai_git_mapping()
            .get(ctx, BonsaisOrGitShas::Bonsai(parents))
            .await?
            .into_iter()
            .map(|entry| entry.bcs_id)
            .collect()
    };
    let (batchable, mut per_commit): (Vec<_>, Vec<_>) = built
        .into_iter()
        .partition(|(_, bcs)| bcs.parents().all(|parent| mapped_parents.contains(&parent)));
    let unbatchable_commits = per_commit.len();

    // The batch is concurrent inside the manager, so calls run one at a time.
    let mut mapped: HashMap<ChangesetId, MappedGitCommitId> = HashMap::new();
    let mut samples: Vec<Duration> = Vec::new();
    for batch in batchable.chunks(LOCAL_DERIVE_BATCH_MAX) {
        let started = Instant::now();
        let csids: Vec<ChangesetId> = batch
            .iter()
            .map(|(_, bcs)| bcs.get_changeset_id())
            .collect();
        match derive_batch(ctx, repo, csids).await {
            Ok(batch_mapped) => {
                let per_commit = started.elapsed() / batch_mapped.len() as u32;
                samples.extend(std::iter::repeat_n(per_commit, batch_mapped.len()));
                mapped.extend(batch_mapped);
            }
            Err(e) => {
                ctx.scuba()
                    .clone()
                    .add("log_tag", "local_batch_git_derivation_failed")
                    .add("commits", batch.len())
                    .add("error", format!("{e:#}"))
                    .unsampled()
                    .log();
                per_commit.extend(batch.iter().cloned());
            }
        }
    }
    let fallback_commits = per_commit.len() - unbatchable_commits;
    let (fallback_prepared, fallback_samples) =
        derive_per_commit(ctx, repo, per_commit, concurrency).await?;
    samples.extend(fallback_samples);
    mapped.extend(
        fallback_prepared
            .into_iter()
            .map(|prepared| (prepared.new_cs, prepared.mapped_git)),
    );

    let prepared = order
        .into_iter()
        .map(|(old_cs, new_cs)| {
            let mapped_git = mapped
                .get(&new_cs)
                .cloned()
                .ok_or_else(|| anyhow!("no git id derived for {new_cs}"))?;
            Ok(PreparedManifestCommit {
                old_cs,
                new_cs,
                mapped_git,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Derived {
        prepared,
        samples,
        unbatchable_commits,
        fallback_commits,
    })
}

/// One local batch over the still-underived commits. The manager only
/// returns a duration and persists whatever the derivable's `derive_batch`
/// hands back without checking it covers every input, so the ids are read
/// back from the mapping, and a gap is an error here so the caller's
/// per-commit fallback covers it instead of the land failing later.
async fn derive_batch<R>(
    ctx: &CoreContext,
    repo: &R,
    csids: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, MappedGitCommitId>>
where
    R: RepoDerivedDataRef,
{
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_underived_batch::<MappedGitCommitId>(ctx, csids.clone(), None)
        .await?;
    let mapped = manager
        .fetch_derived_batch::<MappedGitCommitId>(ctx, csids.clone(), None)
        .await?;
    if let Some(missing) = csids.iter().find(|csid| !mapped.contains_key(csid)) {
        bail!("git id for {missing} is not in bonsai_git_mapping after batch derivation");
    }
    Ok(mapped)
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
