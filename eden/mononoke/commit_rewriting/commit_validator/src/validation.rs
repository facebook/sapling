/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogEntry;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::get_commit_sync_outcome;
use cross_repo_sync::types::Large;
use cross_repo_sync::types::Small;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use cross_repo_sync::validation::report_different;
use cross_repo_sync::CommitSyncDataProvider;
use cross_repo_sync::CommitSyncOutcome;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::TryFutureExt;
use futures_stats::TimedFutureExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::FileType;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use movers::get_movers;
use movers::Mover;
use reachabilityindex::LeastCommonAncestorsHint;
use ref_cast::RefCast;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use revset::RangeNodeStream;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::error;
use slog::info;
use stats::prelude::*;
use synced_commit_mapping::SqlSyncedCommitMapping;

use crate::reporting::log_validation_result_to_scuba;
use crate::tail::QueueSize;

type LargeBookmarkName = Large<BookmarkName>;
type PathToFileNodeIdMapping = HashMap<MPath, (FileType, HgFileNodeId)>;

define_stats! {
    prefix = "mononoke.commit_validation";
    bizzare_boookmark_move: timeseries(Rate, Sum),
}

/// A struct, representing a single "validation entry":
/// a single commit in the `BookmarkUpdateLogEntry`
#[derive(Clone, PartialEq, Eq)]
pub struct EntryCommitId {
    /// A `BookmarkUpdateLogEntry` id, which introduced this commit
    pub bookmarks_update_log_entry_id: i64,
    /// An index of this commit in the "parent" `BookmarkUpdateLogEntry`
    commit_in_entry: i64,
    /// Total number of commits, introduced by the `BookmarkUpdateLogEntry`
    /// Needed to provide convenient logging and verify if commit is the
    /// last in the `BookmarkUpdateLogEntry`
    total_commits_in_entry: usize,
}

impl EntryCommitId {
    pub fn last_commit_for_bookmark_move(&self) -> bool {
        self.commit_in_entry == (self.total_commits_in_entry as i64) - 1
    }
}

impl Debug for EntryCommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Entry {} ({}/{})",
            self.bookmarks_update_log_entry_id,
            // 1-indexing to be clear to humans
            self.commit_in_entry + 1,
            self.total_commits_in_entry
        )
    }
}

/// Enum, representing a change to a single `MPath` in
/// a `FullManifestDiff`
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FilenodeDiffPayload {
    Added(FileType, HgFileNodeId),
    Removed,
    ChangedTo(FileType, HgFileNodeId),
}

/// A unit of change in a `FullManifestDiff`
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FilenodeDiff {
    /// An `MPath`, being changed
    mpath: MPath,
    /// A change, happening to the `mpath`
    payload: FilenodeDiffPayload,
}

impl FilenodeDiff {
    fn new(mpath: MPath, payload: FilenodeDiffPayload) -> Self {
        Self { mpath, payload }
    }

    /// Build `FilenodeDiff` from `manifest::Diff`
    fn from_diff(diff: Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>) -> Option<Self> {
        use Diff::*;
        use Entry::*;
        match diff {
            // We don't care about repo root diffs!
            Added(None, _) | Removed(None, _) | Changed(None, _, _) => None,
            // We don't care about tree diffs!
            Added(_, Tree(_)) | Removed(_, Tree(_)) | Changed(_, _, Tree(_)) => None,
            Added(Some(mpath), Leaf((file_type, hg_filenode_id))) => Some(Self::new(
                mpath,
                FilenodeDiffPayload::Added(file_type, hg_filenode_id),
            )),
            Removed(Some(mpath), Leaf(_)) => Some(Self::new(mpath, FilenodeDiffPayload::Removed)),
            Changed(Some(mpath), _, Leaf((file_type, hg_filenode_id))) => Some(Self::new(
                mpath,
                FilenodeDiffPayload::ChangedTo(file_type, hg_filenode_id),
            )),
        }
    }

    fn from_added_file(added_file: (MPath, (FileType, HgFileNodeId))) -> Self {
        let (mpath, (file_type, hg_filenode_id)) = added_file;
        let payload = FilenodeDiffPayload::Added(file_type, hg_filenode_id);
        Self { mpath, payload }
    }

    pub fn apply_mover(self, mover: &Mover) -> Result<Option<Self>, Error> {
        let Self { mpath, payload } = self;
        mover(&mpath).map(|maybe_moved_mpath| {
            maybe_moved_mpath.map(|moved_mpath| Self::new(moved_mpath, payload))
        })
    }

    pub fn as_tuple(&self) -> (&MPath, &FilenodeDiffPayload) {
        let Self {
            ref mpath,
            ref payload,
        } = self;
        (mpath, payload)
    }
}

/// A set of changes between two commit manifests, computed
/// by running `ManifestOps::Diff`
type FullManifestDiff = HashSet<FilenodeDiff>;

/// An auxillary struct, containing helper methods for sync
/// validation between a large repo and a single small repo
#[derive(Clone)]
struct ValidationHelper {
    large_repo: Large<BlobRepo>,
    small_repo: Small<BlobRepo>,
    scuba_sample: MononokeScubaSampleBuilder,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
}

impl ValidationHelper {
    fn new(
        large_repo: Large<BlobRepo>,
        small_repo: Small<BlobRepo>,
        scuba_sample: MononokeScubaSampleBuilder,
        live_commit_sync_config: CfgrLiveCommitSyncConfig,
    ) -> Self {
        Self {
            large_repo,
            small_repo,
            scuba_sample,
            live_commit_sync_config,
        }
    }

    async fn get_synced_commit(
        &self,
        ctx: CoreContext,
        hash: Large<ChangesetId>,
        mapping: &SqlSyncedCommitMapping,
    ) -> Result<Option<(Small<ChangesetId>, CommitSyncConfigVersion)>, Error> {
        let large_repo_id = self.large_repo.0.get_repoid();
        let small_repo_id = self.small_repo.0.get_repoid();

        let commit_sync_data_provider =
            CommitSyncDataProvider::Live(Arc::new(self.live_commit_sync_config.clone()));
        let maybe_commit_sync_outcome: Option<CommitSyncOutcome> = get_commit_sync_outcome(
            &ctx,
            Source(large_repo_id),
            Target(small_repo_id),
            Source(hash.0),
            mapping,
            CommitSyncDirection::LargeToSmall,
            &commit_sync_data_provider,
        )
        .await?;

        use CommitSyncOutcome::*;
        Ok(match maybe_commit_sync_outcome {
            None | Some(NotSyncCandidate(_)) | Some(EquivalentWorkingCopyAncestor(_, _)) => None,
            Some(RewrittenAs(cs_id, version_name)) => Some((Small(cs_id), version_name)),
        })
    }

    fn move_full_manifest_diff_large_to_small(
        &self,
        full_manifest_diff: Large<FullManifestDiff>,
        large_to_small_mover: &Mover,
    ) -> Result<Large<FullManifestDiff>, Error> {
        let moved_fmd: Result<FullManifestDiff, Error> = full_manifest_diff
            .0
            .into_iter()
            .flat_map(|filenode_diff| filenode_diff.apply_mover(large_to_small_mover).transpose())
            .collect();
        Ok(Large(moved_fmd?))
    }

    /// Check if `payload` is a noop change against p1 of the current commit
    ///
    /// Because of differences of how Mononoke and Mercurial generate filenodes
    /// for merge commits, it is possible that one backend reuses an existing
    /// `HgFilenodeId`, while the other backend generates a new one, while using
    /// the existing one as a parent. A practical case when this may happen arises
    /// from the fact that during a push Mononoke preserves a provided filenode,
    /// while during a x-repo sync it generates a new one.
    /// So the following situation is possible:
    ///
    /// A    ->  A'
    /// |\   ->  | \
    /// B C  ->  B' C'
    ///
    /// -> represents a x-repo sync
    /// A and B share the same `HgFilenodeId` for some `MPath`, while
    /// A' and B' have different `HgFilenodeId`s. At the same time, in all
    /// cases the actual `ContentId` and `FileType` is the same!
    async fn is_true_change(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        p1_root_mf_id: &HgManifestId,
        mpath: &MPath,
        payload: &FilenodeDiffPayload,
    ) -> Result<bool, Error> {
        debug!(
            ctx.logger(),
            "Checking if a change to {:?}'s filenode in {} ({:?}) relects a true change",
            mpath,
            repo.name(),
            payload
        );

        use FilenodeDiffPayload::*;
        match payload {
            Added(_, _) | Removed => Ok(true),
            ChangedTo(new_file_type, new_filenode_id) => {
                let maybe_entry_in_p1 = p1_root_mf_id
                    .find_entry(ctx.clone(), repo.blobstore().clone(), Some(mpath.clone()))
                    .await?;

                let (p1_file_type, p1_filenode_id) = match maybe_entry_in_p1 {
                    None => {
                        // p1 does not even have an entry at this path, definitely
                        // something fishy (we really should not have gotten ChangedTo
                        // in the first place).
                        return Err(format_err!(
                            "parent's manifest lacks a file at {:?}, but we were told the diff is Changed, not Added",
                            mpath
                        ));
                    }
                    Some(Entry::Tree(_)) => {
                        // p1 has this path as a directory, it's a true change
                        debug!(
                            ctx.logger(),
                            "change at {:?} in {} was from a tree to a file, so a true change",
                            mpath,
                            repo.name()
                        );
                        return Ok(true);
                    }
                    Some(Entry::Leaf((p1_file_type, p1_filenode_id))) => {
                        (p1_file_type, p1_filenode_id)
                    }
                };

                if p1_file_type != *new_file_type {
                    debug!(
                        ctx.logger(),
                        "change at {:?} in {} was to a file type: {:?}->{:?}",
                        mpath,
                        repo.name(),
                        p1_file_type,
                        new_file_type,
                    );
                    return Ok(true);
                }

                let (p1_content_id, new_content_id) = try_join!(
                    async {
                        let e = p1_filenode_id.load(ctx, repo.blobstore()).await?;
                        Result::<_, Error>::Ok(e.content_id())
                    },
                    async {
                        let e = new_filenode_id.load(ctx, repo.blobstore()).await?;
                        Result::<_, Error>::Ok(e.content_id())
                    }
                )?;

                if p1_content_id != new_content_id {
                    debug!(
                        ctx.logger(),
                        "change at {:?} in {} resolved to different ContentIds: {:?}->{:?}",
                        mpath,
                        repo.name(),
                        p1_content_id,
                        new_content_id,
                    );

                    Ok(true)
                } else {
                    debug!(
                        ctx.logger(),
                        "change at {:?} in {} resolved to identical ContentIds",
                        mpath,
                        repo.name(),
                    );

                    Ok(false)
                }
            }
        }
    }

    /// Remove noop `FilenodeDiffPayload` changes from `paths_and_payloads`
    /// See the docstring of `is_true_change` for more info.
    async fn filter_out_noop_filenode_id_changes(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: &ChangesetId,
        paths_and_payloads: Vec<(MPath, &FilenodeDiffPayload)>,
    ) -> Result<Vec<MPath>, Error> {
        let maybe_p1 = repo
            .get_changeset_parents_by_bonsai(ctx.clone(), cs_id.clone())
            .await?
            .first()
            .cloned();

        let p1 = match maybe_p1 {
            Some(p1) => p1,
            None => {
                // `cs_id` is a root commit, such commits cannot have "fake" filenode changes
                // Just return all paths
                return Ok(paths_and_payloads.into_iter().map(|(mp, _)| mp).collect());
            }
        };

        let p1_root_mf_id = fetch_root_mf_id(ctx, repo, p1).await?;

        // Note: this loop will only ever be entered during the cases when
        // two repos have non-empty symmetric difference of their `FullManifestDiff`s
        // In other words: extremely rarely. Therefore keeping it sequential is fine
        // for now. We can improve it if it ever becomes a problem.
        let mut ret = Vec::new();
        for (mpath, payload) in paths_and_payloads {
            if self
                .is_true_change(ctx, repo, &p1_root_mf_id, &mpath, payload)
                .await?
            {
                ret.push(mpath);
            } else {
                info!(
                    ctx.logger(),
                    "Skipping a change to {:?} as the filenode change is deemed false", mpath
                );
            }
        }

        Ok(ret)
    }

    async fn filter_out_large_noop_filenode_id_changes(
        &self,
        ctx: &CoreContext,
        large_cs_id: &Large<ChangesetId>,
        paths_and_payloads: Vec<(Large<MPath>, Large<&FilenodeDiffPayload>)>,
    ) -> Result<Vec<Large<MPath>>, Error> {
        let v: Vec<_> = self
            .filter_out_noop_filenode_id_changes(
                ctx,
                &self.large_repo.0,
                &large_cs_id.0,
                paths_and_payloads
                    .into_iter()
                    .map(|(mp, pld)| (mp.0, pld.0))
                    .collect(),
            )
            .await?
            .into_iter()
            .map(Large)
            .collect();

        Ok(v)
    }

    async fn filter_out_small_noop_filenode_id_changes(
        &self,
        ctx: &CoreContext,
        small_cs_id: &Small<ChangesetId>,
        paths_and_payloads: Vec<(Small<MPath>, Small<&FilenodeDiffPayload>)>,
    ) -> Result<Vec<Small<MPath>>, Error> {
        let v: Vec<_> = self
            .filter_out_noop_filenode_id_changes(
                ctx,
                &self.small_repo.0,
                &small_cs_id.0,
                paths_and_payloads
                    .into_iter()
                    .map(|(mp, pld)| (mp.0, pld.0))
                    .collect(),
            )
            .await?
            .into_iter()
            .map(Small)
            .collect();

        Ok(v)
    }
}

/// An auxillary struct, containing helper methods for sync
/// validation a large repo and potentially multiple small repos
#[derive(Clone)]
pub struct ValidationHelpers {
    large_repo: Large<InnerRepo>,
    helpers: HashMap<Small<RepositoryId>, ValidationHelper>,
    /// The "master" bookmark in the large repo. This is needed when
    /// we are unfolding an entry, which creates a new bookmark. Such entry
    /// does not have `from_changeset_id`, so instead we using the `to_changeset_id % master`
    /// revset to unfold an entry.
    large_repo_master_bookmark: BookmarkName,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
    mapping: SqlSyncedCommitMapping,
}

impl ValidationHelpers {
    pub fn new(
        large_repo: InnerRepo,
        helpers: HashMap<
            RepositoryId,
            (Large<BlobRepo>, Small<BlobRepo>, MononokeScubaSampleBuilder),
        >,
        large_repo_master_bookmark: BookmarkName,
        mapping: SqlSyncedCommitMapping,
        live_commit_sync_config: CfgrLiveCommitSyncConfig,
    ) -> Self {
        Self {
            large_repo: Large(large_repo),
            helpers: helpers
                .into_iter()
                .map(|(repo_id, (large_repo, small_repo, scuba_sample))| {
                    (
                        Small(repo_id),
                        ValidationHelper::new(
                            large_repo,
                            small_repo,
                            scuba_sample,
                            live_commit_sync_config.clone(),
                        ),
                    )
                })
                .collect(),
            large_repo_master_bookmark,
            live_commit_sync_config,
            mapping,
        }
    }

    fn get_helper(&self, repo_id: &Small<RepositoryId>) -> Result<&ValidationHelper, Error> {
        self.helpers
            .get(repo_id)
            .ok_or_else(|| format_err!("Repo {} is not present in ValidationHelpers", repo_id))
    }

    fn get_small_repo(&self, repo_id: &Small<RepositoryId>) -> Result<&Small<BlobRepo>, Error> {
        let helper = self.get_helper(repo_id)?;
        Ok(&helper.small_repo)
    }

    // For a given commit, find all repos into which this commit was rewritten,
    // the resulting `ChangesetId` values, and the renamed bookmark
    async fn get_synced_commits(
        &self,
        ctx: &CoreContext,
        hash: Large<ChangesetId>,
    ) -> Result<HashMap<Small<RepositoryId>, (Small<ChangesetId>, CommitSyncConfigVersion)>, Error>
    {
        let commit_sync_outcomes = try_join_all(self.helpers.iter().map(|(repo_id, helper)| {
            let mapping = &self.mapping;
            async move {
                let maybe_synced_commit = helper
                    .get_synced_commit(ctx.clone(), hash.clone(), mapping)
                    .await?;

                Result::<_, Error>::Ok(
                    maybe_synced_commit.map(|synced_commit| (repo_id.clone(), synced_commit)),
                )
            }
        }))
        .await?
        .into_iter()
        .filter_map(std::convert::identity)
        .collect();

        debug!(
            ctx.logger(),
            "Commit {} is rewritten as follows: {:?}", hash, commit_sync_outcomes
        );
        Ok(commit_sync_outcomes)
    }

    async fn get_root_full_manifest_diff(
        ctx: CoreContext,
        repo: &BlobRepo,
        root_mf_id: HgManifestId,
    ) -> Result<FullManifestDiff, Error> {
        let all_files = list_all_filenode_ids(ctx, repo, root_mf_id).await?;
        Ok(all_files
            .into_iter()
            .map(FilenodeDiff::from_added_file)
            .collect())
    }

    /// Produce a full manifest diff between `cs_id` and its first parent
    async fn get_full_manifest_diff(
        ctx: CoreContext,
        repo: &BlobRepo,
        cs_id: &ChangesetId,
    ) -> Result<FullManifestDiff, Error> {
        let cs_root_mf_id_fut = fetch_root_mf_id(&ctx, repo, cs_id.clone());
        let maybe_p1 = repo
            .get_changeset_parents_by_bonsai(ctx.clone(), cs_id.clone())
            .await?
            .first()
            .cloned();

        let p1 = match maybe_p1 {
            Some(p1) => p1,
            None => {
                info!(
                    ctx.logger(),
                    "{} is a root cs. Grabbing its entire manifest", cs_id
                );
                return Self::get_root_full_manifest_diff(
                    ctx.clone(),
                    repo,
                    cs_root_mf_id_fut.await?,
                )
                .await;
            }
        };

        let p1_root_mf_id_fut = fetch_root_mf_id(&ctx, repo, p1);
        let (cs_root_mf_id, p1_root_mf_id): (HgManifestId, HgManifestId) =
            try_join!(cs_root_mf_id_fut, p1_root_mf_id_fut)?;

        let r: Vec<Result<_, Error>> = p1_root_mf_id
            .diff(ctx, repo.get_blobstore(), cs_root_mf_id)
            .filter_map(|diff| async move { diff.map(FilenodeDiff::from_diff).transpose() })
            .collect()
            .await;

        r.into_iter().collect()
    }

    async fn get_large_repo_full_manifest_diff(
        &self,
        ctx: CoreContext,
        cs_id: &Large<ChangesetId>,
    ) -> Result<Large<FullManifestDiff>, Error> {
        Self::get_full_manifest_diff(ctx, &self.large_repo.0.blob_repo, &cs_id.0)
            .await
            .map(Large)
    }

    async fn get_small_repos_full_manifest_diffs(
        &self,
        ctx: CoreContext,
        repo_cs_ids: HashMap<Small<RepositoryId>, (Small<ChangesetId>, CommitSyncConfigVersion)>,
    ) -> Result<
        HashMap<
            Small<RepositoryId>,
            (
                Small<ChangesetId>,
                Small<FullManifestDiff>,
                CommitSyncConfigVersion,
            ),
        >,
        Error,
    > {
        let full_manifest_diff_futures =
            repo_cs_ids
                .into_iter()
                .map(|(small_repo_id, (small_cs_id, version_name))| {
                    cloned!(ctx);
                    async move {
                        let small_repo = self.get_small_repo(&small_repo_id)?;
                        let full_manifest_diff =
                            Self::get_full_manifest_diff(ctx, &small_repo.0, &small_cs_id.0)
                                .await?;
                        Result::<_, Error>::Ok((
                            small_repo_id,
                            (small_cs_id, Small(full_manifest_diff), version_name),
                        ))
                    }
                });

        let full_manifest_diff: Vec<(
            Small<RepositoryId>,
            (
                Small<ChangesetId>,
                Small<FullManifestDiff>,
                CommitSyncConfigVersion,
            ),
        )> = try_join_all(full_manifest_diff_futures).await?;

        Ok(full_manifest_diff.into_iter().collect())
    }

    async fn get_large_repo_master(&self, ctx: CoreContext) -> Result<ChangesetId, Error> {
        let maybe_cs_id = self
            .large_repo
            .0
            .blob_repo
            .bookmarks()
            .get(ctx, &self.large_repo_master_bookmark)
            .await?;
        maybe_cs_id.ok_or_else(|| format_err!("No master in the large repo"))
    }

    // First returned mover is small to large, second is large to small
    async fn create_movers(
        &self,
        small_repo_id: &Small<RepositoryId>,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<(Mover, Mover), Error> {
        let commit_sync_config = self
            .live_commit_sync_config
            .get_commit_sync_config_by_version(
                self.large_repo.0.blob_repo.get_repoid(),
                version_name,
            )
            .await?;

        let movers = get_movers(
            &commit_sync_config,
            small_repo_id.0,
            CommitSyncDirection::SmallToLarge,
        )?;
        Ok((movers.mover, movers.reverse_mover))
    }
}

/// This represents a single commit move after `BookmarkUpdateLogEntry` is unfolded
#[derive(Debug)]
pub struct CommitEntry {
    entry_id: EntryCommitId,
    bookmark_name: BookmarkName,
    cs_id: ChangesetId,
    queue_size: QueueSize,
}

/// Given a `BookmarkUpdateLogEntry`, create a stream of `CommitEntry` objects,
/// one for each commit, introduced by this entry
/// This will return an empty stream for entry, which deletes a bookmark.
/// For entry, which creates a bookmark, an equivalent of a revset `new_book % master`
/// will be returned.
pub async fn unfold_bookmarks_update_log_entry(
    ctx: &CoreContext,
    (entry, queue_size): (BookmarkUpdateLogEntry, QueueSize),
    validation_helpers: &ValidationHelpers,
) -> Result<impl Stream<Item = Result<CommitEntry, Error>>, Error> {
    let bookmarks_update_log_entry_id = entry.id;
    let changeset_fetcher = validation_helpers
        .large_repo
        .0
        .blob_repo
        .get_changeset_fetcher();
    let lca_hint = validation_helpers.large_repo.0.skiplist_index.clone();
    let is_master_entry = entry.bookmark_name == validation_helpers.large_repo_master_bookmark;
    let master_cs_id = validation_helpers
        .get_large_repo_master(ctx.clone())
        .await?;

    info!(
        ctx.logger(),
        "BookmarkUpdateLogEntry {} will be expanded into commits", bookmarks_update_log_entry_id,
    );

    // Normally a single entry would resolve to just a few commits.
    // Even if we imagine a case, where a single entry will produce
    // a stream of a million commits (merge of a new repo or smth like this)
    // we are talking about <200bytes * 1M ~= 200Mb, which it not that big
    // of a deal
    let mut collected = match (
        entry.from_changeset_id.clone(),
        entry.to_changeset_id.clone(),
    ) {
        (_, None) => {
            // Entry deletes a bookmark. Nothing to validate
            vec![]
        }
        (None, Some(to_cs_id)) => {
            // Entry creates a bookmark (or it is a blobimport entry,
            // or it has been created by the mononoke_admin in the
            // original repo).
            // In any case, out best bet is to build up all
            // commits since LCA(to_cs_id, master)

            if is_master_entry {
                // A bizzare case, when a master bookmark is either created
                // or blobiported. I do not think we will ever observe this in
                // practice, but for completeness sake, let's just make sure
                // we verify `to_cs_id` itself. If we decided to just use
                // the revset from below, it would've excluded `to_cs_id`
                // as it is likely to be an ancestor of master.
                STATS::bizzare_boookmark_move.add_value(1);
                vec![Ok(to_cs_id)]
            } else {
                info!(
                    ctx.logger(),
                    "[{}] Creation of a new bookmark at {}, slow path.",
                    bookmarks_update_log_entry_id,
                    to_cs_id
                );
                // This might be slow. If too many bookmakrs are being created, it can be optimised
                // or we could just check to_cs_id as a best effort.
                DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                    ctx.clone(),
                    &changeset_fetcher,
                    lca_hint,
                    vec![to_cs_id],
                    vec![master_cs_id],
                )
                .compat()
                .collect()
                .await
            }
        }
        (Some(from_cs_id), Some(to_cs_id)) => {
            if !lca_hint
                .is_ancestor(ctx, &changeset_fetcher, from_cs_id, to_cs_id)
                .await?
            {
                info!(
                    ctx.logger(),
                    "[{}] {} -> {} not a forward move, slow path.",
                    bookmarks_update_log_entry_id,
                    from_cs_id,
                    to_cs_id
                );
                // This might be slow. If too many bookmakrs are being non-FF moved, it can be optimised
                // or we could just check to_cs_id as a best effort.
                DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                    ctx.clone(),
                    &changeset_fetcher,
                    lca_hint,
                    vec![to_cs_id],
                    vec![from_cs_id],
                )
                .compat()
                .collect()
                .await
            } else {
                let mut v: Vec<_> =
                    RangeNodeStream::new(ctx.clone(), changeset_fetcher, from_cs_id, to_cs_id)
                        .compat()
                        .collect()
                        .await;
                // Drop from_cs
                v.pop();
                v
            }
        }
    };
    // Changesets are listed top to bottom, let's reverse
    collected.reverse();

    let total_commits_in_entry = collected.len();
    info!(
        ctx.logger(),
        "BookmarkUpdateLogEntry {} has been expanded into {} commits",
        bookmarks_update_log_entry_id,
        total_commits_in_entry
    );

    let bookmark_name = entry.bookmark_name;
    Ok(stream::iter(collected).map_ok({
        cloned!(bookmark_name);
        let mut commit_in_entry = 0;
        move |cs_id| {
            commit_in_entry += 1;

            let entry_id = EntryCommitId {
                bookmarks_update_log_entry_id,
                commit_in_entry: commit_in_entry - 1,
                total_commits_in_entry,
            };

            CommitEntry {
                entry_id,
                bookmark_name: bookmark_name.clone(),
                cs_id,
                queue_size,
            }
        }
    }))
}

/// A preprocessed-entry struct, which has enough data for 2 purposes:
/// - to decide if entry `id` needs to be skipped
/// - to create an `EntryPreparedForValidation`
#[derive(Debug)]
pub struct CommitEntryWithSmallReposMapped {
    entry_id: EntryCommitId,
    bookmark_name: LargeBookmarkName,
    cs_id: Large<ChangesetId>,
    small_repo_cs_ids: HashMap<Small<RepositoryId>, (Small<ChangesetId>, CommitSyncConfigVersion)>,
    queue_size: QueueSize,
}

pub async fn get_entry_with_small_repo_mapings(
    ctx: &CoreContext,
    entry: CommitEntry,
    validation_helpers: &ValidationHelpers,
) -> Result<Option<CommitEntryWithSmallReposMapped>, Error> {
    info!(
        ctx.logger(),
        "Mapping small cs_ids for entry {:?}; book: {}; to_cs_id: {:?}; remaining queue: {}",
        entry.entry_id,
        entry.bookmark_name,
        entry.cs_id,
        entry.queue_size.0,
    );
    let cs_id = Large(entry.cs_id.clone());

    let small_repo_cs_ids: HashMap<
        Small<RepositoryId>,
        (Small<ChangesetId>, CommitSyncConfigVersion),
    > = validation_helpers
        .get_synced_commits(ctx, cs_id.clone())
        .await?;

    Ok(Some(CommitEntryWithSmallReposMapped {
        entry_id: entry.entry_id.clone(),
        bookmark_name: Large(entry.bookmark_name),
        cs_id,
        small_repo_cs_ids,
        queue_size: entry.queue_size,
    }))
}

#[derive(Debug)]
pub struct EntryPreparedForValidation {
    pub entry_id: EntryCommitId,
    cs_id: Large<ChangesetId>,
    large_repo_full_manifest_diff: Large<FullManifestDiff>,
    small_repo_full_manifest_diffs: HashMap<
        Small<RepositoryId>,
        (
            Small<ChangesetId>,
            Small<FullManifestDiff>,
            CommitSyncConfigVersion,
        ),
    >,
    queue_size: QueueSize,
    preparation_duration: Duration,
}

pub async fn prepare_entry(
    ctx: &CoreContext,
    entry_with_small_repo_mappings: CommitEntryWithSmallReposMapped,
    validation_helpers: &ValidationHelpers,
) -> Result<Option<EntryPreparedForValidation>, Error> {
    let CommitEntryWithSmallReposMapped {
        entry_id,
        bookmark_name,
        cs_id,
        small_repo_cs_ids,
        queue_size,
    } = entry_with_small_repo_mappings;
    let cs_id = cs_id.clone();

    let before_preparation = std::time::Instant::now();
    info!(
        ctx.logger(),
        "Preparing entry {:?}; book: {}; cs_id: {:?}; remaining queue: {}",
        entry_id,
        bookmark_name,
        cs_id,
        queue_size.0,
    );

    // Note: executing the following two async operations  concurrently
    // does not really matter as we are already executing multiple
    // entry preparations at the same time
    let (large_repo_full_manifest_diff, small_repo_full_manifest_diffs) = try_join!(
        validation_helpers.get_large_repo_full_manifest_diff(ctx.clone(), &cs_id),
        validation_helpers.get_small_repos_full_manifest_diffs(ctx.clone(), small_repo_cs_ids)
    )?;

    Ok(Some(EntryPreparedForValidation {
        entry_id,
        cs_id,
        large_repo_full_manifest_diff,
        small_repo_full_manifest_diffs,
        queue_size,
        preparation_duration: before_preparation.elapsed(),
    }))
}

/// Validate that parents of a changeset in a small repo are
/// ancestors of it's equivalent in the large repo
async fn validate_topological_order<'a>(
    ctx: &'a CoreContext,
    large_repo: &'a Large<BlobRepo>,
    large_cs_id: Large<ChangesetId>,
    small_repo: &'a Small<BlobRepo>,
    small_cs_id: Small<ChangesetId>,
    large_repo_lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    mapping: &'a SqlSyncedCommitMapping,
    commit_sync_data_provider: &'a CommitSyncDataProvider,
) -> Result<(), Error> {
    debug!(
        ctx.logger(),
        "validating topological order for {}<->{}", large_cs_id, small_cs_id
    );
    let small_repo_id = small_repo.0.get_repoid();
    let large_repo_id = large_repo.0.get_repoid();

    let small_parents = small_repo
        .0
        .get_changeset_parents_by_bonsai(ctx.clone(), small_cs_id.0.clone())
        .await?;

    let remapped_small_parents: Vec<(ChangesetId, ChangesetId)> =
        try_join_all(small_parents.into_iter().map(|small_parent| {
            cloned!(ctx, commit_sync_data_provider);
            async move {
                let maybe_commit_sync_outcome = get_commit_sync_outcome(
                    &ctx,
                    Source(small_repo_id),
                    Target(large_repo_id),
                    Source(small_parent),
                    mapping,
                    CommitSyncDirection::SmallToLarge,
                    &commit_sync_data_provider,
                )
                .await?;

                let commit_sync_outcome = maybe_commit_sync_outcome.ok_or_else(|| {
                    format_err!(
                        "Unexpectedly missing CommitSyncOutcome for {} in {}->{}",
                        small_parent,
                        small_repo_id,
                        large_repo_id,
                    )
                })?;

                use CommitSyncOutcome::*;
                let remapping_of_small_parent = match commit_sync_outcome {
                    RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => cs_id,
                    NotSyncCandidate(_) => {
                        return Err(format_err!(
                            "Parent of synced {} is NotSyncCandidate in {}->{}",
                            small_cs_id,
                            small_repo_id,
                            large_repo_id,
                        ));
                    }
                };
                Ok((small_parent, remapping_of_small_parent))
            }
        }))
        .await?;

    let large_repo_fetcher = large_repo.0.get_changeset_fetcher();
    try_join_all(remapped_small_parents.into_iter().map(
        |(small_parent, remapping_of_small_parent)| {
            cloned!(ctx, large_repo_lca_hint, large_repo_fetcher);
            async move {
                let is_ancestor = large_repo_lca_hint
                    .is_ancestor(
                        &ctx,
                        &large_repo_fetcher,
                        remapping_of_small_parent,
                        large_cs_id.0.clone(),
                    )
                    .await?;

                if !is_ancestor {
                    Err(format_err!(
                        "{} (remapping of parent {} of {} in {}) is not an ancestor of {} in {}",
                        remapping_of_small_parent,
                        small_parent,
                        small_cs_id,
                        small_repo_id,
                        large_cs_id,
                        large_repo_id,
                    ))
                } else {
                    Ok(())
                }
            }
        },
    ))
    .await?;

    debug!(
        ctx.logger(),
        "done validating topological order for {}<->{}", large_cs_id, small_cs_id
    );
    Ok(())
}

/// Given changes to equivalent `MPath`s in two synced repos,
/// return the differences between the changes
fn compare_diffs_at_mpath(
    mpath: &MPath,
    small_payload: Small<&FilenodeDiffPayload>,
    large_payload: Large<&FilenodeDiffPayload>,
) -> (
    Option<(MPath, Small<HgFileNodeId>, Large<HgFileNodeId>)>,
    Option<(MPath, Small<FileType>, Large<FileType>)>,
    Option<(
        MPath,
        Small<FilenodeDiffPayload>,
        Large<FilenodeDiffPayload>,
    )>,
) {
    use FilenodeDiffPayload::*;
    let mut should_be_equivalent = None;
    let mut different_filetypes = None;
    let mut different_actions = None;

    match (small_payload.0, large_payload.0) {
        (Added(small_file_type, small_file_node), Added(large_file_type, large_file_node))
        | (
            ChangedTo(small_file_type, small_file_node),
            ChangedTo(large_file_type, large_file_node),
        ) => {
            if small_file_node != large_file_node {
                should_be_equivalent = Some((
                    mpath.clone(),
                    Small(small_file_node.clone()),
                    Large(large_file_node.clone()),
                ));
            }

            if small_file_type != large_file_type {
                different_filetypes = Some((
                    mpath.clone(),
                    Small(small_file_type.clone()),
                    Large(large_file_type.clone()),
                ));
            }
        }
        (Removed, Removed) => {} // both are `Removed` so there's nothing to compare
        (small_action, large_action) => {
            different_actions = Some((
                mpath.clone(),
                Small(small_action.clone()),
                Large(large_action.clone()),
            ));
        }
    }

    (should_be_equivalent, different_filetypes, different_actions)
}

/// Given full manifest diffs, check that they represent
/// equivalent sets of changes in two repos
async fn validate_full_manifest_diffs_equivalence<'a>(
    ctx: &'a CoreContext,
    validation_helper: &'a ValidationHelper,
    large_cs_id: &'a Large<ChangesetId>,
    small_cs_id: &'a Small<ChangesetId>,
    large_repo_full_manifest_diff: Large<FullManifestDiff>,
    small_repo_full_manifest_diff: Small<FullManifestDiff>,
    small_to_large_mover: Mover,
    large_to_small_mover: Mover,
) -> Result<(), Error> {
    let moved_large_repo_full_manifest_diff = validation_helper
        .move_full_manifest_diff_large_to_small(
            large_repo_full_manifest_diff,
            &large_to_small_mover,
        )?;

    // Let's remove all `FilenodeDiff` structs, which are strictly equivalent
    // Note that `in_small_but_not_in_large` and `in_large_but_not_in_small`
    // do *not* represent different `MPath`s, but `FilenodeDiff` structs. That is, it
    // is possible that there are two different `FilenodeDiff` structs in the
    // large and small repos, but they correspond to the same `MPath` and may
    // even represent the exact same `ContentId`!
    let in_small_but_not_in_large: HashMap<Small<&MPath>, Small<&FilenodeDiffPayload>> =
        small_repo_full_manifest_diff
            .0
            .difference(&moved_large_repo_full_manifest_diff.0)
            .map(|filenode_diff| {
                let (mpath, payload) = filenode_diff.as_tuple();
                (Small(mpath), Small(payload))
            })
            .collect();

    // Note that this hashmap maps small paths to large payloads. This is
    // intentional and represents the idea that paths are already moved
    let mut in_large_but_not_in_small: HashMap<Small<&MPath>, Large<&FilenodeDiffPayload>> =
        moved_large_repo_full_manifest_diff
            .0
            .difference(&small_repo_full_manifest_diff.0)
            .map(|filenode_diff| {
                let (mpath, payload) = filenode_diff.as_tuple();
                (Small(mpath), Large(payload))
            })
            .collect();

    // This is set of *small* repo changes, which do not have an equivalent
    // in the large repo
    let mut missing_in_large_repo: Vec<(Small<MPath>, Small<&FilenodeDiffPayload>)> = vec![];

    let mut should_be_equivalent: Vec<(MPath, Small<HgFileNodeId>, Large<HgFileNodeId>)> = vec![];
    let mut different_filetypes: Vec<(MPath, Small<FileType>, Large<FileType>)> = vec![];
    let mut different_actions: Vec<(
        MPath,
        Small<FilenodeDiffPayload>,
        Large<FilenodeDiffPayload>,
    )> = vec![];

    for (small_mpath, small_payload) in in_small_but_not_in_large {
        if small_to_large_mover(small_mpath.0)?.is_none() {
            // It is expected to be missing in the large repo
            continue;
        };

        match in_large_but_not_in_small.remove(&small_mpath) {
            None => {
                // `small_mpath` is present in the small repo, `small_to_large_mover`
                // does not rewrite it into `None`. Seems like a problem!
                let small_mpath: Small<MPath> = small_mpath.cloned();
                missing_in_large_repo.push((small_mpath, small_payload));
            }
            Some(large_payload) => {
                let (
                    maybe_should_be_equivalent,
                    maybe_different_filetypes,
                    maybe_different_actions,
                ) = compare_diffs_at_mpath(small_mpath.0, small_payload, large_payload);

                should_be_equivalent.extend(maybe_should_be_equivalent);
                different_filetypes.extend(maybe_different_filetypes);
                different_actions.extend(maybe_different_actions);
            }
        };
    }

    // This is a set of *large* repo changes, which do not have an equivalent
    // in the small repo. We need large paths to filter noop ones from these.
    let missing_in_small_repo: Vec<(Large<MPath>, Large<&FilenodeDiffPayload>)> =
        in_large_but_not_in_small
            .into_iter()
            .map(
                |(small_mpath, payload): (Small<&MPath>, Large<&FilenodeDiffPayload>)| {
                    // `in_large_but_not_in_small` was initially populated by small
                    // paths, so let's convert
                    let large_mpath = small_to_large_mover(small_mpath.0)?.ok_or_else(|| {
                        format_err!(
                            "{:?} unexpectedly produces None when moved small-to-large",
                            small_mpath.0
                        )
                    })?;

                    Result::<_, Error>::Ok((Large(large_mpath), payload))
                },
            )
            .collect::<Result<Vec<_>, Error>>()?;

    let missing_in_small_repo: Vec<Large<MPath>> = validation_helper
        .filter_out_large_noop_filenode_id_changes(ctx, large_cs_id, missing_in_small_repo)
        .await?;

    // Note: we populated `missing_in_small_repo` with large repo paths,
    // and `missing_in_large_repo` with small repo paths. Let's unify this
    // to small repo paths, so that we always report the same kind of path.
    let missing_in_large_repo: Vec<Large<MPath>> = validation_helper
        .filter_out_small_noop_filenode_id_changes(ctx, small_cs_id, missing_in_large_repo)
        .await?
        .into_iter()
        .map(|small_mpath: Small<MPath>| {
            let large_mpath = small_to_large_mover(&small_mpath.0)?.ok_or_else(|| {
                format_err!(
                    "{:?} surprisingly produces None when moved small-to-large",
                    small_mpath
                )
            })?;

            Ok(Large(large_mpath))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let small_name = validation_helper.small_repo.0.name();
    let large_name = validation_helper.large_repo.0.name();

    report_missing(
        ctx,
        missing_in_large_repo,
        large_cs_id,
        small_name,
        large_name,
    )?;
    report_missing(
        ctx,
        missing_in_small_repo,
        large_cs_id,
        large_name,
        small_name,
    )?;

    // In order to call fns, provided by cross_repo_sync/src/validation,
    // we are switching perspective from Large/Small to Source/Taret
    let source_cs_id = Source(large_cs_id.0);
    let source_repo = Source::ref_cast(&validation_helper.large_repo.0);
    let target_repo = Target::ref_cast(&validation_helper.small_repo.0);
    let source_name = Source(large_name);
    let target_name = Target(small_name);

    let different_filetypes = small_large_to_source_target(different_filetypes);
    report_different(
        ctx,
        different_filetypes,
        &source_cs_id,
        "filetype",
        source_name,
        target_name,
    )?;
    let different_actions = small_large_to_source_target(different_actions);
    report_different(
        ctx,
        different_actions,
        &source_cs_id,
        "action",
        source_name,
        target_name,
    )?;

    let should_be_equivalent = small_large_to_source_target(should_be_equivalent);
    verify_filenodes_have_same_contents(
        ctx,
        target_repo,
        source_repo,
        &source_cs_id,
        should_be_equivalent,
    )
    .await?;

    Ok(())
}

/// Rewrap a vector of things, assuming that `Small` is a `Target`,
/// and `Large` is a `Source`
fn small_large_to_source_target<T, P>(
    v: Vec<(T, Small<P>, Large<P>)>,
) -> Vec<(T, Source<P>, Target<P>)> {
    v.into_iter()
        .map(|(t, small, large)| (t, Source(large.0), Target(small.0)))
        .collect()
}

/// Report file changes, present in acommit in one repo,
/// but missing in a commit in another repo
fn report_missing(
    ctx: &CoreContext,
    missing_things: Vec<Large<MPath>>,
    large_cs_id: &Large<ChangesetId>,
    where_present: &str,
    where_missing: &str,
) -> Result<(), Error> {
    if !missing_things.is_empty() {
        for missing_thing in missing_things.iter().take(10) {
            debug!(
                ctx.logger(),
                "A change to {:?} is present in {}, but missing in {} (large repo cs {})",
                missing_thing,
                where_present,
                where_missing,
                large_cs_id,
            );
        }

        return Err(format_err!(
            "Found {} changes missing in {}, but present in {} (large repo cs {})",
            missing_things.len(),
            where_missing,
            where_present,
            large_cs_id,
        ));
    }

    Ok(())
}

async fn validate_in_a_single_repo(
    ctx: CoreContext,
    validation_helper: ValidationHelper,
    large_repo: Large<BlobRepo>,
    large_cs_id: Large<ChangesetId>,
    small_cs_id: Small<ChangesetId>,
    large_repo_full_manifest_diff: Large<FullManifestDiff>,
    small_repo_full_manifest_diff: Small<FullManifestDiff>,
    large_repo_lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    small_to_large_mover: Mover,
    large_to_small_mover: Mover,
    mapping: SqlSyncedCommitMapping,
) -> Result<(), Error> {
    validate_full_manifest_diffs_equivalence(
        &ctx,
        &validation_helper,
        &large_cs_id,
        &small_cs_id,
        large_repo_full_manifest_diff,
        small_repo_full_manifest_diff,
        small_to_large_mover,
        large_to_small_mover,
    )
    .await?;

    validate_topological_order(
        &ctx,
        &large_repo,
        large_cs_id,
        &validation_helper.small_repo,
        small_cs_id,
        large_repo_lca_hint,
        &mapping,
        &CommitSyncDataProvider::Live(Arc::new(validation_helper.live_commit_sync_config.clone())),
    )
    .await
}

pub async fn validate_entry(
    ctx: &CoreContext,
    prepared_entry: EntryPreparedForValidation,
    validation_helpers: &ValidationHelpers,
) -> Result<(), Error> {
    let EntryPreparedForValidation {
        entry_id,
        cs_id: large_cs_id,
        large_repo_full_manifest_diff,
        small_repo_full_manifest_diffs,
        queue_size,
        preparation_duration,
    } = prepared_entry;

    let large_repo_lca_hint = &validation_helpers.large_repo.0.skiplist_index;
    let small_repo_validation_futs = small_repo_full_manifest_diffs.into_iter().map(
        |(repo_id, (small_cs_id, small_repo_full_manifest_diff, version_name))| {
            cloned!(large_repo_full_manifest_diff, large_repo_lca_hint);

            let entry_id = &entry_id;
            let large_repo = &validation_helpers.large_repo;
            let large_cs_id = &large_cs_id;

            async move {
                let validation_helper = validation_helpers
                    .helpers
                    .get(&repo_id)
                    .ok_or_else(|| format_err!("small repo {} not found", repo_id))?
                    .clone();
                let scuba_sample = validation_helper.scuba_sample.clone();
                let mapping = &validation_helpers.mapping;
                let (small_to_large_mover, large_to_small_mover) = validation_helpers
                    .create_movers(&repo_id, &version_name)
                    .await?;

                let (stats, validation_result): (_, Result<Result<(), _>, tokio::task::JoinError>) =
                    tokio::task::spawn({
                        cloned!(ctx, small_cs_id, large_cs_id, large_repo, mapping);
                        async move {
                            validate_in_a_single_repo(
                                ctx,
                                validation_helper,
                                Large(large_repo.blob_repo.clone()),
                                large_cs_id,
                                small_cs_id,
                                large_repo_full_manifest_diff,
                                small_repo_full_manifest_diff,
                                large_repo_lca_hint,
                                small_to_large_mover,
                                large_to_small_mover,
                                mapping,
                            )
                            .await
                        }
                    })
                    .timed()
                    .await;

                // `validation_result` is a `Result`of `spawn`, the `Ok`
                // of which contains the `Result` of `validate_in_a_single_repo`
                let validation_result = validation_result?;
                let validation_duration = stats.completion_time;
                let maybe_error_str = match validation_result.as_ref() {
                    Ok(()) => None,
                    Err(e) => {
                        error!(
                            ctx.logger(),
                            "Error while verifying against {}: {:?}", version_name, e
                        );
                        Some(format!("{}", e))
                    }
                };
                log_validation_result_to_scuba(
                    scuba_sample,
                    entry_id.bookmarks_update_log_entry_id,
                    large_cs_id,
                    &small_cs_id,
                    maybe_error_str,
                    queue_size,
                    preparation_duration,
                    validation_duration,
                );

                validation_result.map_err(Error::from)
            }
        },
    );
    try_join_all(small_repo_validation_futs).await?;
    info!(ctx.logger(), "Validated entry: {:?}", entry_id);
    Ok(())
}

async fn fetch_root_mf_id(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo.derive_hg_changeset(ctx, cs_id).await?;
    let changeset = hg_cs_id.load(ctx, repo.blobstore()).await?;
    Ok(changeset.manifestid())
}

async fn list_all_filenode_ids(
    ctx: CoreContext,
    repo: &BlobRepo,
    mf_id: HgManifestId,
) -> Result<PathToFileNodeIdMapping, Error> {
    let repoid = repo.get_repoid();
    info!(
        ctx.logger(),
        "fetching filenode ids for {:?} in {}", mf_id, repoid,
    );
    let res = mf_id
        .list_all_entries(ctx.clone(), repo.get_blobstore())
        .try_filter_map(move |(path, entry)| {
            let res = match entry {
                Entry::Leaf(leaf_payload) => path.map(|path| (path, leaf_payload)),
                Entry::Tree(_) => None,
            };
            future::ready(Ok(res))
        })
        .try_collect::<HashMap<_, _>>()
        .await?;

    debug!(
        ctx.logger(),
        "fetched {} filenode ids for {}",
        res.len(),
        repoid,
    );
    Ok(res)
}

async fn verify_filenodes_have_same_contents<
    // item is a tuple: (MPath, large filenode id, small filenode id)
    I: IntoIterator<Item = (MPath, Source<HgFileNodeId>, Target<HgFileNodeId>)>,
>(
    ctx: &CoreContext,
    target_repo: &Target<BlobRepo>,
    source_repo: &Source<BlobRepo>,
    source_hash: &Source<ChangesetId>,
    should_be_equivalent: I,
) -> Result<(), Error> {
    let fetched_content_ids = stream::iter(should_be_equivalent)
        .map({
            move |(path, source_filenode_id, target_filenode_id)| async move {
                debug!(
                    ctx.logger(),
                    "checking content for different filenodes: source {} vs target {}",
                    source_filenode_id,
                    target_filenode_id,
                );
                let f1 = async move {
                    source_filenode_id
                        .0
                        .load(ctx, source_repo.0.blobstore())
                        .await
                }
                .map_ok(|e| Source(e.content_id()));
                let f2 = async move {
                    target_filenode_id
                        .0
                        .load(ctx, target_repo.0.blobstore())
                        .await
                }
                .map_ok(|e| Target(e.content_id()));

                let (c1, c2) = future::try_join(f1, f2).await?;
                Result::<_, Error>::Ok((path, c1, c2))
            }
        })
        .buffered(1000)
        .try_collect::<Vec<_>>()
        .await?;

    let different_contents: Vec<_> = fetched_content_ids
        .into_iter()
        .filter(|(_mpath, c1, c2)| c1.0 != c2.0)
        .collect();

    report_different(
        ctx,
        different_contents,
        source_hash,
        "contents",
        Source(source_repo.0.name()),
        Target(target_repo.0.name()),
    )
}

#[cfg(test)]
mod tests {
    use cross_repo_sync::update_mapping_with_version;
    use cross_repo_sync_test_utils::init_small_large_repo;
    use cross_repo_sync_test_utils::xrepo_mapping_version_with_small_repo;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use skiplist::SkiplistIndex;
    use tests_utils::CommitIdentifier;
    use tests_utils::CreateCommitContext;
    use tokio::runtime::Runtime;

    use super::*;

    async fn add_commits_to_repo(
        ctx: &CoreContext,
        spec: Vec<HashMap<&str, &str>>,
        repo: &BlobRepo,
    ) -> Result<Vec<ChangesetId>, Error> {
        let mut parent: CommitIdentifier = "master".into();
        let mut commits: Vec<ChangesetId> = vec![];
        for file_changes in spec {
            let commit = CreateCommitContext::new(ctx, repo, vec![parent])
                .add_files(file_changes)
                .commit()
                .await?;
            commits.push(commit.clone());
            parent = commit.into();
        }

        Ok(commits)
    }

    /// Initialize small and large repos, create commits according
    /// to provided specifications  and update the SyncedCommitMapping.
    /// `large_repo_commits_spec`, `small_repo_commits_spec` are specs for new commits.
    /// Each spec will trigger a linear chain of commits in its repo with
    /// appropriate file changes.
    /// `commit_index_mapping` is a mapping specification, which says:
    /// "for each pair of indices `i=>j` in this hashmap, please take
    /// i-th produced commit in the large repo and map to j'th produced
    /// commit in the small repo.
    async fn test_topological_order_validation(
        fb: FacebookInit,
        large_repo_commits_spec: Vec<HashMap<&str, &str>>,
        small_repo_commits_spec: Vec<HashMap<&str, &str>>,
        commit_index_mapping: HashMap<usize, usize>,
        large_index_to_test: usize,
        small_index_to_test: usize,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
        let small_to_large_commit_syncer = syncers.small_to_large;
        let small_repo = small_to_large_commit_syncer.get_small_repo();
        let large_repo = small_to_large_commit_syncer.get_large_repo();
        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let large_commits = add_commits_to_repo(&ctx, large_repo_commits_spec, large_repo).await?;
        let small_commits = add_commits_to_repo(&ctx, small_repo_commits_spec, small_repo).await?;
        let commit_mapping: HashMap<ChangesetId, ChangesetId> = commit_index_mapping
            .into_iter()
            .map(|(large_index, small_index)| {
                (
                    small_commits[small_index].clone(),
                    large_commits[large_index].clone(),
                )
            })
            .collect();

        update_mapping_with_version(
            &ctx,
            commit_mapping,
            &small_to_large_commit_syncer,
            &xrepo_mapping_version_with_small_repo(),
        )
        .await?;
        let large_repo = Large(large_repo.clone());
        let small_repo = Small(small_repo.clone());

        validate_topological_order(
            &ctx,
            &large_repo,
            Large(large_commits[large_index_to_test].clone()),
            &small_repo,
            Small(small_commits[small_index_to_test].clone()),
            lca_hint,
            &small_to_large_commit_syncer.mapping,
            small_to_large_commit_syncer.get_commit_sync_data_provider(),
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    fn test_topological_order_validation_ok(fb: FacebookInit) -> Result<(), Error> {
        // Large repo  Mapping  Small repo
        //    2    -------------    1
        //    |                     |
        //    1                     |
        //    |                     |
        //    0    -------------    0
        //   ..                     ..
        let runtime = Runtime::new()?;
        runtime.block_on(async {
            let validation_result = test_topological_order_validation(
                fb,
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile1" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                hashmap! {0 => 0, 2 => 1},
                2,
                1,
            )
            .await;
            assert!(validation_result.is_ok());
            Ok(())
        })
    }

    #[fbinit::test]
    fn test_topological_order_validation_bad_direct_parents(fb: FacebookInit) -> Result<(), Error> {
        // Large repo  Mapping  Small repo
        //    1      --\   /--      1
        //    |         \ /         |
        //    |          X          |
        //    |         / \         |
        //    0      --/   \--      0
        //   ..                     ..
        let runtime = Runtime::new()?;
        runtime.block_on(async {
            let validation_result = test_topological_order_validation(
                fb,
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile1" => "newcontent"},
                ],
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile1" => "newcontent"},
                ],
                hashmap! {0 => 1, 1 => 0},
                0,
                1,
            )
            .await;
            assert!(validation_result.is_err());
            Ok(())
        })
    }

    #[fbinit::test]
    fn test_topological_order_validation_bad_ancestors(fb: FacebookInit) -> Result<(), Error> {
        // Large repo  Mapping  Small repo
        //    2      --\   /--      1
        //    |         \ /         |
        //    1          X          |
        //    |         / \         |
        //    0      --/   \--      0
        //   ..                     ..
        let runtime = Runtime::new()?;
        runtime.block_on(async {
            let validation_result = test_topological_order_validation(
                fb,
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile1" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                hashmap! {0 => 1, 2 => 0},
                0,
                1,
            )
            .await;
            assert!(validation_result.is_err());
            Ok(())
        })
    }

    #[fbinit::test]
    fn test_topological_order_validation_bad_unremapped_ancestors(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Large repo  Mapping  Small repo
        //    2      ---------      1
        //    |                     |
        //    1                     |
        //    |                     |
        //    0                     0
        //   ..                     ..
        let runtime = Runtime::new()?;
        runtime.block_on(async {
            let validation_result = test_topological_order_validation(
                fb,
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile1" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                vec![
                    hashmap! {"newfile0" => "newcontent"},
                    hashmap! {"newfile2" => "newcontent"},
                ],
                hashmap! {2 => 1},
                2,
                1,
            )
            .await;
            assert!(validation_result.is_err());
            Ok(())
        })
    }
}
