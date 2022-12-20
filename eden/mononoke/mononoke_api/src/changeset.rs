/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;

use anyhow::anyhow;
use basename_suffix_skeleton_manifest::RootBasenameSuffixSkeletonManifest;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use changesets::ChangesetsRef;
use chrono::DateTime;
use chrono::FixedOffset;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data::BonsaiDerived;
use derived_data_manager::BonsaiDerivable;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_lazy_shared::LazyShared;
use hooks::CrossRepoPushSource;
use hooks::HookOutcome;
use hooks::PushAuthoredBy;
use itertools::EitherOrBoth;
use manifest::Diff as ManifestDiff;
use manifest::Entry as ManifestEntry;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;
use maplit::hashset;
use mercurial_types::Globalrev;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
pub use mononoke_types::Generation;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::SkeletonManifestId;
use mononoke_types::Svnrev;
use reachabilityindex::ReachabilityIndex;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataArc;
use skeleton_manifest::RootSkeletonManifestId;
use sorted_vector_map::SortedVectorMap;
use tunables::tunables;
use unodes::RootUnodeManifestId;
use vec1::Vec1;

use crate::changeset_path::ChangesetPathContentContext;
use crate::changeset_path::ChangesetPathContext;
use crate::changeset_path::ChangesetPathHistoryContext;
use crate::changeset_path_diff::ChangesetPathDiffContext;
use crate::errors::MononokeError;
use crate::path::is_related_to;
use crate::path::MononokePath;
use crate::repo::RepoContext;
use crate::specifiers::ChangesetId;
use crate::specifiers::GitSha1;
use crate::specifiers::HgChangesetId;

#[derive(Clone, Debug)]
enum PathMutableHistory {
    /// Checking the mutable history datastore shows no changes
    NoChange,
    /// Change of parent and path
    PathAndChangeset(ChangesetId, MononokePath),
}

impl PathMutableHistory {
    /// Get the mutable parents (if any) of this path
    pub fn get_parent_cs_id(&self) -> Option<ChangesetId> {
        match self {
            Self::NoChange => None,
            Self::PathAndChangeset(cs_id, _) => Some(*cs_id),
        }
    }

    /// Is this path overridden by mutable history?
    fn is_override(&self) -> bool {
        match self {
            Self::NoChange => false,
            Self::PathAndChangeset(_, _) => true,
        }
    }

    /// Extract the copy_from information relating to this entry, if any
    fn get_copy_from(&self) -> Option<(ChangesetId, &MononokePath)> {
        match self {
            Self::NoChange => None,
            Self::PathAndChangeset(cs_id, path) => Some((*cs_id, path)),
        }
    }
}

#[derive(Clone)]
pub struct ChangesetContext {
    repo: RepoContext,
    id: ChangesetId,
    bonsai_changeset: LazyShared<Result<BonsaiChangeset, MononokeError>>,
    changeset_info: LazyShared<Result<ChangesetInfo, MononokeError>>,
    root_unode_manifest_id: LazyShared<Result<RootUnodeManifestId, MononokeError>>,
    root_fsnode_id: LazyShared<Result<RootFsnodeId, MononokeError>>,
    root_skeleton_manifest_id: LazyShared<Result<RootSkeletonManifestId, MononokeError>>,
    root_deleted_manifest_v2_id: LazyShared<Result<RootDeletedManifestV2Id, MononokeError>>,
    root_basename_suffix_skeleton_manifest:
        LazyShared<Result<RootBasenameSuffixSkeletonManifest, MononokeError>>,
    /// None if no mutable history, else map from supplied paths to data fetched
    mutable_history: Option<HashMap<MononokePath, PathMutableHistory>>,
}

#[derive(Default)]
pub struct ChangesetHistoryOptions {
    pub until_timestamp: Option<i64>,
    pub descendants_of: Option<ChangesetId>,
    pub exclude_changeset_and_ancestors: Option<ChangesetId>,
}

#[derive(Clone)]
pub enum ChangesetFileOrdering {
    Unordered,
    Ordered { after: Option<MononokePath> },
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangesetDiffItem {
    TREES,
    FILES,
}

impl fmt::Debug for ChangesetContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetContext(repo={:?} id={:?})",
            self.repo().name(),
            self.id()
        )
    }
}

fn to_vec1<X>(maybe_vec: Option<Vec<X>>) -> Option<Vec1<X>> {
    maybe_vec.and_then(|v| Vec1::try_from_vec(v).ok())
}

/// A context object representing a query to a particular commit in a repo.
impl ChangesetContext {
    /// Construct a new `MononokeChangeset`.  The changeset must exist
    /// in the repo.
    pub(crate) fn new(repo: RepoContext, id: ChangesetId) -> Self {
        let bonsai_changeset = LazyShared::new_empty();
        let changeset_info = LazyShared::new_empty();
        let root_unode_manifest_id = LazyShared::new_empty();
        let root_fsnode_id = LazyShared::new_empty();
        let root_skeleton_manifest_id = LazyShared::new_empty();
        let root_deleted_manifest_v2_id = LazyShared::new_empty();
        let root_basename_suffix_skeleton_manifest = LazyShared::new_empty();
        Self {
            repo,
            id,
            changeset_info,
            bonsai_changeset,
            root_unode_manifest_id,
            root_fsnode_id,
            root_skeleton_manifest_id,
            root_deleted_manifest_v2_id,
            root_basename_suffix_skeleton_manifest,
            mutable_history: None,
        }
    }

    /// The context for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        self.repo.ctx()
    }

    /// Adds copy information from mutable renames as an override to replace
    /// the Bonsai copy information
    pub async fn add_mutable_renames(
        &mut self,
        paths: impl Iterator<Item = MononokePath>,
    ) -> Result<(), MononokeError> {
        let mutable_renames = &self.repo.mutable_renames();
        let ctx = self.repo.ctx();
        let cs_id = self.id;

        let copy_info = stream::iter(paths.map(move |path| async move {
            let maybe_rename_entry = mutable_renames
                .get_rename(ctx, cs_id, path.as_mpath().cloned())
                .await?;
            let rename = match maybe_rename_entry {
                Some(entry) => {
                    let cs_id = entry.src_cs_id();
                    let path = MononokePath::new(entry.src_path().cloned());
                    PathMutableHistory::PathAndChangeset(cs_id, path)
                }
                None => PathMutableHistory::NoChange,
            };
            Ok::<_, MononokeError>((path, rename))
        }))
        .buffered(100)
        .try_collect()
        .await?;

        self.mutable_history = Some(copy_info);
        Ok(())
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    /// Deconstruct the changeset into RepoContext and ChangesetId.
    pub fn into_repo_and_id(self) -> (RepoContext, ChangesetId) {
        let Self { repo, id, .. } = self;
        (repo, id)
    }

    /// The Mercurial ID for the changeset.
    pub async fn hg_id(&self) -> Result<Option<HgChangesetId>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), self.id)
            .await?;
        Ok(mapping.get(0).map(|(hg_cs_id, _)| *hg_cs_id))
    }

    /// The Globalrev for the changeset.
    pub async fn globalrev(&self) -> Result<Option<Globalrev>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .bonsai_globalrev_mapping()
            .get_globalrev_from_bonsai(self.ctx(), self.id)
            .await?;
        Ok(mapping.into_iter().next())
    }

    /// The SVN revision number for the changeset.
    pub async fn svnrev(&self) -> Result<Option<Svnrev>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .bonsai_svnrev_mapping()
            .get_svnrev_from_bonsai(self.ctx(), self.id)
            .await?;
        Ok(mapping)
    }

    /// The git Sha1 for the changeset (if available).
    pub async fn git_sha1(&self) -> Result<Option<GitSha1>, MononokeError> {
        Ok(self
            .repo()
            .blob_repo()
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(self.ctx(), self.id)
            .await?)
    }

    /// Derive a derivable data type for this changeset.
    // Desugared async syntax so we can return a future with static lifetime.
    fn derive<Derivable: BonsaiDerivable>(
        &self,
    ) -> impl Future<Output = Result<Derivable, MononokeError>> + Send + 'static {
        let ctx = self.ctx().clone();
        let repo_derived_data = self.repo.blob_repo().repo_derived_data_arc();
        let id = self.id;
        async move {
            repo_derived_data
                .derive::<Derivable>(&ctx, id)
                .await
                .map_err(MononokeError::from)
        }
    }

    pub(crate) async fn root_unode_manifest_id(
        &self,
    ) -> Result<RootUnodeManifestId, MononokeError> {
        self.root_unode_manifest_id
            .get_or_init(|| self.derive::<RootUnodeManifestId>())
            .await
    }

    pub(crate) async fn root_fsnode_id(&self) -> Result<RootFsnodeId, MononokeError> {
        self.root_fsnode_id
            .get_or_init(|| self.derive::<RootFsnodeId>())
            .await
    }

    pub(crate) async fn root_basename_suffix_skeleton_manifest(
        &self,
    ) -> Result<RootBasenameSuffixSkeletonManifest, MononokeError> {
        self.root_basename_suffix_skeleton_manifest
            .get_or_init(|| self.derive::<RootBasenameSuffixSkeletonManifest>())
            .await
    }

    pub(crate) async fn root_skeleton_manifest_id(
        &self,
    ) -> Result<RootSkeletonManifestId, MononokeError> {
        self.root_skeleton_manifest_id
            .get_or_init(|| self.derive::<RootSkeletonManifestId>())
            .await
    }

    pub(crate) async fn root_deleted_manifest_v2_id(
        &self,
    ) -> Result<RootDeletedManifestV2Id, MononokeError> {
        self.root_deleted_manifest_v2_id
            .get_or_init(|| self.derive::<RootDeletedManifestV2Id>())
            .await
    }

    /// Query the root directory in the repository at this changeset revision.
    pub async fn root(&self) -> Result<ChangesetPathContentContext, MononokeError> {
        ChangesetPathContentContext::new(self.clone(), None).await
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    ///
    /// Returns a path content context, which is a context suitable for
    /// queries about the content at this path.
    pub async fn path_with_content<P>(
        &self,
        path: P,
    ) -> Result<ChangesetPathContentContext, MononokeError>
    where
        P: TryInto<MononokePath>,
        MononokeError: From<P::Error>,
    {
        ChangesetPathContentContext::new(self.clone(), path.try_into()?).await
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    ///
    /// Returns a path history context, which is a context suitable for
    /// queries about the history of this path.
    pub async fn path_with_history<P>(
        &self,
        path: P,
    ) -> Result<ChangesetPathHistoryContext, MononokeError>
    where
        P: TryInto<MononokePath>,
        MononokeError: From<P::Error>,
    {
        ChangesetPathHistoryContext::new(self.clone(), path.try_into()?).await
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    ///
    /// Returns a path context, which is a context that is only suitable for
    /// queries about the type of item that exists at this path.
    ///
    /// If you need to query the content or history of a path, use
    /// `path_with_content` or `path_with_history` instead.
    pub async fn path<P>(&self, path: P) -> Result<ChangesetPathContext, MononokeError>
    where
        P: TryInto<MononokePath>,
        MononokeError: From<P::Error>,
    {
        ChangesetPathContext::new(self.clone(), path.try_into()?).await
    }

    /// Returns a stream of path history contexts for a set of paths.
    ///
    /// This performs an efficient manifest traversal, and as such returns
    /// contexts only for **paths which exist**.
    pub async fn paths_with_history(
        &self,
        paths: impl Iterator<Item = MononokePath>,
    ) -> Result<impl Stream<Item = Result<ChangesetPathHistoryContext, MononokeError>>, MononokeError>
    {
        Ok(self
            .root_unode_manifest_id()
            .await?
            .manifest_unode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                paths.map(|path| path.into_mpath()),
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathHistoryContext::new_with_unode_entry(
                        changeset.clone(),
                        MononokePath::new(mpath),
                        entry,
                    )
                }
            }))
    }

    /// Returns a stream of path content contexts for a set of paths.
    ///
    /// This performs an efficient manifest traversal, and as such returns
    /// contexts only for **paths which exist**.
    pub async fn paths_with_content(
        &self,
        paths: impl Iterator<Item = MononokePath>,
    ) -> Result<impl Stream<Item = Result<ChangesetPathContentContext, MononokeError>>, MononokeError>
    {
        Ok(self
            .root_fsnode_id()
            .await?
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                paths.map(|path| path.into_mpath()),
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    cloned!(changeset);
                    async move {
                        ChangesetPathContentContext::new_with_fsnode_entry(
                            changeset.clone(),
                            MononokePath::new(mpath),
                            entry,
                        )
                        .await
                    }
                }
            }))
    }

    /// Returns a stream of path contexts for a set of paths.
    ///
    /// This performs an efficient manifest traversal, and as such returns
    /// contexts only for **paths which exist**.
    pub async fn paths(
        &self,
        paths: impl Iterator<Item = MononokePath>,
    ) -> Result<impl Stream<Item = Result<ChangesetPathContext, MononokeError>>, MononokeError>
    {
        Ok(self
            .root_skeleton_manifest_id()
            .await?
            .skeleton_manifest_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                paths.map(|path| path.into_mpath()),
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathContext::new_with_skeleton_manifest_entry(
                        changeset.clone(),
                        MononokePath::new(mpath),
                        entry,
                    )
                }
            }))
    }

    fn deleted_paths_impl<Root: RootDeletedManifestIdCommon>(
        &self,
        root: Root,
        paths: impl Iterator<Item = MononokePath> + 'static,
    ) -> impl Stream<Item = Result<ChangesetPathHistoryContext, MononokeError>> + '_ {
        root.find_entries(
            self.ctx(),
            self.repo().blob_repo().blobstore(),
            paths.map(|path| path.into_mpath()),
        )
        .map_err(MononokeError::from)
        .and_then({
            let changeset = self.clone();
            move |(mpath, entry)| {
                ChangesetPathHistoryContext::new_with_deleted_manifest::<Root::Manifest>(
                    changeset.clone(),
                    MononokePath::new(mpath),
                    entry,
                )
            }
        })
    }

    /// Returns a stream of path history contexts for a set of paths.
    ///
    /// This performs an efficient manifest traversal, and as such returns
    /// contexts only for **deleted paths which have existed previously**.
    pub async fn deleted_paths(
        &self,
        paths: impl Iterator<Item = MononokePath> + 'static,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathHistoryContext, MononokeError>> + '_,
        MononokeError,
    > {
        Ok(self.deleted_paths_impl(self.root_deleted_manifest_v2_id().await?, paths))
    }

    /// Get the `BonsaiChangeset` information for this changeset.
    async fn bonsai_changeset(&self) -> Result<BonsaiChangeset, MononokeError> {
        self.bonsai_changeset
            .get_or_init(|| {
                let ctx = self.ctx().clone();
                let blobstore = self.repo.blob_repo().repo_blobstore_arc();
                let id = self.id;
                async move { id.load(&ctx, &blobstore).await.map_err(MononokeError::from) }
            })
            .await
    }

    /// Get the `ChangesetInfo` for this changeset.
    async fn changeset_info(&self) -> Result<ChangesetInfo, MononokeError> {
        if self.repo.derive_changeset_info_enabled() {
            self.changeset_info
                .get_or_init(|| self.derive::<ChangesetInfo>())
                .await
        } else {
            let bonsai = self.bonsai_changeset().await?;
            Ok(ChangesetInfo::new(self.id(), bonsai))
        }
    }

    /// The IDs of the parents of the changeset.
    pub async fn parents(&self) -> Result<Vec<ChangesetId>, MononokeError> {
        Ok(self.changeset_info().await?.parents().collect())
    }

    /// The IDs of mutable parents of the changeset, if any.
    ///
    /// The value can be `None` to indicate that we were given a path
    /// to check, but it had no mutable parents of its own.
    ///
    /// Only returns a non-empty set if add_mutable_renames has been called
    pub fn mutable_parents(&self) -> HashSet<Option<ChangesetId>> {
        self.mutable_history
            .as_ref()
            .map_or(HashSet::new(), |info| {
                info.values()
                    .map(PathMutableHistory::get_parent_cs_id)
                    .collect()
            })
    }

    /// The author of the changeset.
    pub async fn author(&self) -> Result<String, MononokeError> {
        Ok(self.changeset_info().await?.author().to_string())
    }

    /// The date the changeset was authored.
    pub async fn author_date(&self) -> Result<DateTime<FixedOffset>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .author_date()
            .as_chrono()
            .clone())
    }

    /// The committer of the changeset.  May be `None` if the committer
    /// is not tracked.
    pub async fn committer(&self) -> Result<Option<String>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .committer()
            .map(|s| s.to_string()))
    }

    /// The date the changeset was committed.  May be `None` if the
    /// committer is not tracked.
    pub async fn committer_date(&self) -> Result<Option<DateTime<FixedOffset>>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .committer_date()
            .map(|d| d.as_chrono().clone()))
    }

    /// The commit message.
    pub async fn message(&self) -> Result<String, MononokeError> {
        Ok(self.changeset_info().await?.message().to_string())
    }

    /// The generation number of the given changeset
    pub async fn generation(&self) -> Result<Generation, MononokeError> {
        Ok(Generation::new(
            self.repo
                .blob_repo()
                .changesets()
                .get(self.ctx().clone(), self.id)
                .await?
                .ok_or_else(|| {
                    MononokeError::NotAvailable(format!(
                        "Generation number missing for {:?}",
                        &self.id
                    ))
                })?
                .gen,
        ))
    }

    /// All commit extras as (name, value) pairs.
    pub async fn extras(&self) -> Result<Vec<(String, Vec<u8>)>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .extra()
            .map(|(name, value)| (name.to_string(), Vec::from(value)))
            .collect())
    }

    /// File changes associated with the commit.
    pub async fn file_changes(&self) -> Result<SortedVectorMap<MPath, FileChange>, MononokeError> {
        let bonsai = self.bonsai_changeset().await?;
        let bonsai = bonsai.into_mut();
        Ok(bonsai.file_changes)
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.  A commit is considered its
    /// own ancestor for the purpose of this call.
    pub async fn is_ancestor_of(&self, other_commit: ChangesetId) -> Result<bool, MononokeError> {
        let segmented_changelog_rollout_pct =
            tunables().get_segmented_changelog_is_ancestor_percentage();
        let use_segmented_changelog =
            ((rand::random::<usize>() % 100) as i64) < segmented_changelog_rollout_pct;
        if use_segmented_changelog {
            let segmented_changelog = self.repo().segmented_changelog();
            // If we have segmeneted changelog enabled...
            if !segmented_changelog.disabled(self.ctx()).await? {
                // ... and it has the answer for us ...
                if let Some(result) = segmented_changelog
                    .is_ancestor(self.ctx(), self.id, other_commit)
                    .await?
                {
                    self.ctx()
                        .perf_counters()
                        .increment_counter(PerfCounterType::SegmentedChangelogServerSideOpsHits);
                    // ... it's cheaper to return it.
                    return Ok(result);
                }
                self.ctx()
                    .perf_counters()
                    .increment_counter(PerfCounterType::SegmentedChangelogServerSideOpsFallbacks);
            }
        }

        let is_ancestor_of = self
            .repo()
            .skiplist_index_arc()
            .query_reachability(
                self.ctx(),
                &self.repo().blob_repo().get_changeset_fetcher(),
                other_commit,
                self.id,
            )
            .await?;
        Ok(is_ancestor_of)
    }

    /// Returns the lowest common ancestor of two commits.
    ///
    /// In case of ambiguity (can happen with multiple merges of the same branches) returns the
    /// common ancestor with lowest id out of those with highest generation number.
    pub async fn common_base_with(
        &self,
        other_commit: ChangesetId,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let lca = self
            .repo()
            .skiplist_index_arc()
            .lca(
                self.ctx().clone(),
                self.repo().blob_repo().get_changeset_fetcher(),
                self.id,
                other_commit,
            )
            .await?;
        Ok(lca.get(0).map(|id| Self::new(self.repo.clone(), *id)))
    }

    pub async fn diff_unordered(
        &self,
        other: &ChangesetContext,
        include_copies_renames: bool,
        path_restrictions: Option<Vec<MononokePath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
    ) -> Result<Vec<ChangesetPathDiffContext>, MononokeError> {
        self.diff(
            other,
            include_copies_renames,
            path_restrictions,
            diff_items,
            ChangesetFileOrdering::Unordered,
            None,
        )
        .await
    }

    /// Returns differences between this changeset and some other changeset.
    ///
    /// `self` is considered the "new" changeset (so files missing there are "Removed")
    /// `other` is considered the "old" changeset (so files missing there are "Added")
    /// `include_copies_renames` is only available for files when diffing commits with its parent
    /// `path_restrictions` if present will narrow down the diff to given paths
    /// `diff_items` what to include in the output (files, dirs or both)
    pub async fn diff(
        &self,
        other: &ChangesetContext,
        include_copies_renames: bool,
        path_restrictions: Option<Vec<MononokePath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
        ordering: ChangesetFileOrdering,
        limit: Option<usize>,
    ) -> Result<Vec<ChangesetPathDiffContext>, MononokeError> {
        // Helper to that checks if a path is within the givien path restrictions
        fn within_restrictions(
            path: &MononokePath,
            path_restrictions: &Option<Vec<MononokePath>>,
        ) -> bool {
            path_restrictions.as_ref().map_or(true, |i| {
                i.iter().any(|path_restriction| {
                    is_related_to(path.as_mpath(), path_restriction.as_mpath())
                })
            })
        }

        // map from from_path to to_paths (there may be multiple copies
        // for each from_path, so this maps to a vector of paths)
        let mut copy_path_map = HashMap::new();
        // map from to_path to from_path
        let mut inv_copy_path_map = HashMap::new();
        let file_changes = self.file_changes().await?;
        // For now we only consider copies when comparing with parent, or using mutable history
        if include_copies_renames
            && (self.mutable_history.is_some() || self.parents().await?.contains(&other.id))
        {
            let mut to_paths = HashSet::new();
            if let Some(overrides) = &self.mutable_history {
                for (dst_path, mutable_history) in overrides {
                    if let Some((cs_id, path)) = mutable_history.get_copy_from() {
                        if cs_id == other.id() {
                            copy_path_map
                                .entry(path.clone())
                                .or_insert_with(Vec::new)
                                .push(dst_path.clone());
                            to_paths.insert(dst_path.clone());
                        }
                    }
                }
            }

            for (to_path, file_change) in file_changes.iter() {
                let to_path = MononokePath::new(Some(to_path.clone()));
                let path_is_overriden = self
                    .mutable_history
                    .as_ref()
                    .and_then(|history_map| {
                        history_map
                            .get(&to_path)
                            .map(PathMutableHistory::is_override)
                    })
                    .unwrap_or(false);
                if path_is_overriden {
                    // Mutable history overrides immutable if present
                    continue;
                }
                match file_change {
                    FileChange::Change(tc) => {
                        if let Some((from_path, csid)) = tc.copy_from() {
                            let from_path = MononokePath::new(Some(from_path.clone()));
                            if *csid == other.id {
                                copy_path_map
                                    .entry(from_path)
                                    .or_insert_with(Vec::new)
                                    .push(to_path.clone());
                                to_paths.insert(to_path);
                            }
                        }
                    }
                    FileChange::Deletion
                    | FileChange::UntrackedDeletion
                    | FileChange::UntrackedChange(_) => {}
                }
            }

            let other_root_fsnode_id = other.root_fsnode_id().await?;

            // Prefetch fsnode entries for all "from paths" so that we don't need
            // to refetch them later
            let from_path_to_mf_entry = other_root_fsnode_id
                .fsnode_id()
                .find_entries(
                    self.ctx().clone(),
                    self.repo().blob_repo().get_blobstore(),
                    copy_path_map.keys().cloned().map(MononokePath::into_mpath),
                )
                .map_ok(|(maybe_from_path, entry)| (MononokePath::new(maybe_from_path), entry))
                .try_collect::<HashMap<_, _>>();

            // At the same time, find out whether the destinations of copies
            // already existed in the parent.
            let to_path_exists_in_parent = other_root_fsnode_id
                .fsnode_id()
                .find_entries(
                    self.ctx().clone(),
                    other.repo().blob_repo().get_blobstore(),
                    to_paths.into_iter().map(MononokePath::into_mpath),
                )
                .map_ok(|(maybe_to_path, _entry)| MononokePath::new(maybe_to_path))
                .try_collect::<HashSet<_>>();

            let (from_path_to_mf_entry, to_path_exists_in_parent) =
                try_join(from_path_to_mf_entry, to_path_exists_in_parent).await?;

            // Filter out copies where the to_path already existed in the
            // parent.  These don't show up as copies in the diff view.
            copy_path_map.retain(|_, to_paths| {
                to_paths.retain(|to_path| !to_path_exists_in_parent.contains(to_path));
                !to_paths.is_empty()
            });

            // Build the inverse copy map (from to_path to from_path),
            // which includes the manifest entry for the from_path.
            for (from_path, to_paths) in copy_path_map.iter() {
                let mf_entry = from_path_to_mf_entry.get(from_path).ok_or_else(|| {
                    MononokeError::from(anyhow!(
                        "internal error: cannot find {:?} in parent commit",
                        from_path
                    ))
                })?;
                for to_path in to_paths {
                    inv_copy_path_map.insert(to_path, (from_path, mf_entry.clone()));
                }
            }
        }

        // set of paths from other that were copied in (not moved)
        // We check if `self` contains paths that were source for copy or move in `other`
        // If self does contain a path, then we consider it to be a copy, otherwise
        // it's a move to the first location it was copied to.
        let copied_paths = self
            .root_fsnode_id()
            .await?
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                copy_path_map.keys().cloned().map(MononokePath::into_mpath),
            )
            .map_ok(|(maybe_from_path, _)| MononokePath::new(maybe_from_path))
            .try_collect::<HashSet<_>>()
            .await?;

        let (self_manifest_root, other_manifest_root) =
            try_join(self.root_fsnode_id(), other.root_fsnode_id()).await?;

        let diff_files = diff_items.contains(&ChangesetDiffItem::FILES);
        let diff_trees = diff_items.contains(&ChangesetDiffItem::TREES);

        let recurse_pruner = {
            cloned!(path_restrictions);
            move |tree_diff: &ManifestDiff<_>| match tree_diff {
                ManifestDiff::Added(path, ..)
                | ManifestDiff::Changed(path, ..)
                | ManifestDiff::Removed(path, ..) => {
                    let path = MononokePath::new(path.clone());
                    within_restrictions(&path, &path_restrictions)
                }
            }
        };

        let diff = match ordering {
            ChangesetFileOrdering::Unordered => {
                // We start from "other" as manfest.diff() is backwards
                other_manifest_root
                    .fsnode_id()
                    .filtered_diff(
                        self.ctx().clone(),
                        self.repo().blob_repo().get_blobstore(),
                        self_manifest_root.fsnode_id().clone(),
                        self.repo().blob_repo().get_blobstore(),
                        Some,
                        recurse_pruner,
                    )
                    .left_stream()
            }
            ChangesetFileOrdering::Ordered { after } => {
                // We start from "other" as manfest.diff() is backwards
                other_manifest_root
                    .fsnode_id()
                    .filtered_diff_ordered(
                        self.ctx().clone(),
                        self.repo().blob_repo().get_blobstore(),
                        self_manifest_root.fsnode_id().clone(),
                        self.repo().blob_repo().get_blobstore(),
                        after.map(MononokePath::into_mpath),
                        Some,
                        recurse_pruner,
                    )
                    .right_stream()
            }
        };

        let change_contexts = diff
            .try_filter_map(|diff_entry| {
                async {
                    let entry = match diff_entry {
                        ManifestDiff::Added(path, entry @ ManifestEntry::Leaf(_)) => {
                            let path = MononokePath::new(path);
                            if !diff_files || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else if let Some((from_path, from_entry)) =
                                inv_copy_path_map.get(&path)
                            {
                                // There's copy information that we can use.
                                if copied_paths.contains(from_path)
                                    || copy_path_map
                                        .get(*from_path)
                                        .and_then(|to_paths| to_paths.first())
                                        != Some(&path)
                                {
                                    // If the source still exists in the current
                                    // commit, or this isn't the first place it
                                    // was copied to, it was a copy.
                                    let from = ChangesetPathContentContext::new_with_fsnode_entry(
                                        other.clone(),
                                        (**from_path).clone(),
                                        *from_entry,
                                    )
                                    .await?;
                                    Some(ChangesetPathDiffContext::Copied(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path,
                                            entry,
                                        )
                                        .await?,
                                        from,
                                    ))
                                } else {
                                    // If it doesn't, and this is the first place
                                    // it was copied to, it was a move.
                                    let from = ChangesetPathContentContext::new_with_fsnode_entry(
                                        other.clone(),
                                        (**from_path).clone(),
                                        *from_entry,
                                    )
                                    .await?;
                                    Some(ChangesetPathDiffContext::Moved(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path,
                                            entry,
                                        )
                                        .await?,
                                        from,
                                    ))
                                }
                            } else {
                                Some(ChangesetPathDiffContext::Added(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path,
                                        entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        ManifestDiff::Removed(path, entry @ ManifestEntry::Leaf(_)) => {
                            let path = MononokePath::new(path);
                            #[allow(clippy::if_same_then_else)]
                            if copy_path_map.get(&path).is_some() {
                                // The file is was moved (not removed), it will be covered by a "Moved" entry.
                                None
                            } else if !diff_files || !within_restrictions(&path, &path_restrictions)
                            {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::Removed(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        other.clone(),
                                        path,
                                        entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        ManifestDiff::Changed(
                            path,
                            from_entry @ ManifestEntry::Leaf(_),
                            to_entry @ ManifestEntry::Leaf(_),
                        ) => {
                            let path = MononokePath::new(path);
                            if !diff_files || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::Changed(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path.clone(),
                                        to_entry,
                                    )
                                    .await?,
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        other.clone(),
                                        path,
                                        from_entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        ManifestDiff::Added(path, entry @ ManifestEntry::Tree(_)) => {
                            let path = MononokePath::new(path);
                            if !diff_trees || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::Added(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path,
                                        entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        ManifestDiff::Removed(path, entry @ ManifestEntry::Tree(_)) => {
                            let path = MononokePath::new(path);
                            if !diff_trees || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::Removed(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path,
                                        entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        ManifestDiff::Changed(
                            path,
                            from_entry @ ManifestEntry::Tree(_),
                            to_entry @ ManifestEntry::Tree(_),
                        ) => {
                            let path = MononokePath::new(path);
                            if !diff_trees || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::Changed(
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path.clone(),
                                        to_entry,
                                    )
                                    .await?,
                                    ChangesetPathContentContext::new_with_fsnode_entry(
                                        other.clone(),
                                        path,
                                        from_entry,
                                    )
                                    .await?,
                                ))
                            }
                        }
                        // We've already covered all practical possiblities as there are no "changed"
                        // between from trees and files as such are represented as removal+addition
                        _ => None,
                    };
                    Ok(entry)
                }
            })
            .take(limit.unwrap_or(usize::MAX))
            .try_collect::<Vec<_>>()
            .await?;
        Ok(change_contexts)
    }

    async fn find_entries(
        &self,
        prefixes: Option<Vec1<MononokePath>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<
        impl Stream<
            Item = Result<(Option<MPath>, ManifestEntry<SkeletonManifestId, ()>), anyhow::Error>,
        >,
        MononokeError,
    > {
        let root = self.root_skeleton_manifest_id().await?;
        let prefixes = match prefixes {
            Some(prefixes) => prefixes
                .into_iter()
                .map(|prefix| PathOrPrefix::Prefix(prefix.into()))
                .collect(),
            None => vec![PathOrPrefix::Prefix(None)],
        };
        let entries = match ordering {
            ChangesetFileOrdering::Unordered => root
                .skeleton_manifest_id()
                .find_entries(
                    self.ctx().clone(),
                    self.repo().blob_repo().get_blobstore(),
                    prefixes,
                )
                .left_stream(),
            ChangesetFileOrdering::Ordered { after } => root
                .skeleton_manifest_id()
                .find_entries_ordered(
                    self.ctx().clone(),
                    self.repo().blob_repo().get_blobstore(),
                    prefixes,
                    after.map(MononokePath::into_mpath),
                )
                .right_stream(),
        };
        Ok(entries)
    }

    pub async fn find_files_unordered(
        &self,
        prefixes: Option<Vec<MononokePath>>,
        basenames: Option<Vec<String>>,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>> + '_, MononokeError> {
        self.find_files(
            prefixes,
            basenames,
            // None for basename_suffixes
            None,
            ChangesetFileOrdering::Unordered,
        )
        .await
    }

    /// Find files after applying filters on the prefix and basename.
    /// A files is returned if the following conditions hold:
    /// - `prefixes` is None, or there is an element of `prefixes` such that the
    ///   element is a prefix of the file path.
    /// - the basename of the file path is in `basenames`, or there is a string
    ///   in `basename_suffixes` that is a suffix of the basename of the file,
    ///   or both `basenames` and `basename_suffixes` are None.
    /// The order that files are returned is based on the parameter `ordering`.
    /// To continue a paginated query, use the parameter `ordering`.
    pub async fn find_files(
        &self,
        prefixes: Option<Vec<MononokePath>>,
        basenames: Option<Vec<String>>,
        basename_suffixes: Option<Vec<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>> + '_, MononokeError> {
        let basenames_and_suffixes = match (to_vec1(basenames), to_vec1(basename_suffixes)) {
            (None, None) => None,
            (Some(basenames), None) => Some(EitherOrBoth::Left(basenames)),
            (None, Some(suffixes)) => Some(EitherOrBoth::Right(suffixes)),
            (Some(basenames), Some(suffixes)) => Some(EitherOrBoth::Both(basenames, suffixes)),
        };
        Ok(match basenames_and_suffixes {
            Some(basenames_and_suffixes)
                if !tunables().get_disable_basename_suffix_skeleton_manifest() =>
            {
                self.find_files_with_bssm(prefixes, basenames_and_suffixes, ordering)
                    .await?
                    .left_stream()
            }
            basenames_and_suffixes => {
                let (basenames, basename_suffixes) = basenames_and_suffixes
                    .map_or((None, None), |b| b.map_any(Some, Some).or_default());
                self.find_files_without_bssm(
                    to_vec1(prefixes),
                    basenames,
                    basename_suffixes,
                    ordering,
                )
                .await?
                .right_stream()
            }
        })
    }

    pub(crate) async fn find_files_with_bssm(
        &self,
        prefixes: Option<Vec<MononokePath>>,
        basenames_and_suffixes: EitherOrBoth<Vec1<String>, Vec1<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>> + '_, MononokeError> {
        Ok(self
            .root_basename_suffix_skeleton_manifest()
            .await?
            .find_files_filter_basenames(
                self.ctx(),
                self.repo().blob_repo().get_blobstore(),
                prefixes
                    .unwrap_or_else(Vec::new)
                    .into_iter()
                    .map(MononokePath::into_mpath)
                    .collect(),
                basenames_and_suffixes,
                match ordering {
                    ChangesetFileOrdering::Unordered => None,
                    ChangesetFileOrdering::Ordered { after } => {
                        Some(after.map(MononokePath::into_mpath))
                    }
                },
            )
            .await
            .map_err(MononokeError::from)?
            .map(|r| match r {
                Ok(p) => Ok(MononokePath::new(p)),
                Err(err) => Err(MononokeError::from(err)),
            }))
    }

    pub(crate) async fn find_files_without_bssm(
        &self,
        prefixes: Option<Vec1<MononokePath>>,
        basenames: Option<Vec1<String>>,
        basename_suffixes: Option<Vec1<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>>, MononokeError> {
        // First, find the entries, and filter by file prefix.
        let entries = self.find_entries(prefixes, ordering).await?;
        let mpaths = entries.try_filter_map(|(path, entry)| async move {
            match (path, entry) {
                (Some(mpath), ManifestEntry::Leaf(_)) => Ok(Some(mpath)),
                _ => Ok(None),
            }
        });

        // Now, construct a set of basenames to include.
        // These basenames are of type MPathElement rather than being strings.
        let basenames_as_mpath_elements_set = match basenames {
            Some(basenames) => Some(
                basenames
                    .into_iter()
                    .map(|basename| MPathElement::new(basename.into()))
                    .collect::<Result<HashSet<_>, _>>()
                    .map_err(MononokeError::from)?,
            ),
            None => None,
        };

        // Now, filter by basename. We use "left_stream" and "right_stream" to
        // satisfy the type checker, because filtering a stream creates a
        // different "type". Using left and right streams creates an Either type
        // which satisfies the type checker.
        let mpaths = match (basenames_as_mpath_elements_set, basename_suffixes) {
            // If basenames and suffixes are provided, include basenames in
            // the set basenames_as_mpath_elements_set as well as basenames
            // with a suffix in basename_suffixes.
            (Some(basenames_as_mpath_elements_set), Some(basename_suffixes)) => mpaths
                .try_filter(move |mpath| {
                    let basename = mpath.basename();
                    future::ready(
                        basenames_as_mpath_elements_set.contains(basename)
                            || basename_suffixes
                                .iter()
                                .any(|suffix| basename.has_suffix(suffix.as_bytes())),
                    )
                })
                .left_stream()
                .left_stream(),
            // If no suffixes are provided, only match on basenames that are
            // in the set.
            (Some(basenames_as_mpath_elements_set), None) => mpaths
                .try_filter(move |mpath| {
                    future::ready(basenames_as_mpath_elements_set.contains(mpath.basename()))
                })
                .left_stream()
                .right_stream(),
            (None, Some(basename_suffixes)) =>
            // If only suffixes are provided, match on basenames that have a
            // suffix in basename_suffixes.
            {
                mpaths
                    .try_filter(move |mpath| {
                        let basename = mpath.basename();
                        future::ready(
                            basename_suffixes
                                .iter()
                                .any(|suffix| basename.has_suffix(suffix.as_bytes())),
                        )
                    })
                    .right_stream()
                    .left_stream()
            }
            // Otherwise, there are no basename filters, so do not filter.
            (None, None) => mpaths.right_stream().right_stream(),
        };

        Ok(mpaths
            .map_ok(|mpath| MononokePath::new(Some(mpath)))
            .map_err(MononokeError::from))
    }

    /// Returns a stream of `ChangesetContext` for the history of the repository from this commit.
    pub async fn history(
        &self,
        opts: ChangesetHistoryOptions,
    ) -> impl Stream<Item = Result<ChangesetContext, MononokeError>> + '_ {
        let descendants_of = opts
            .descendants_of
            .map(|id| Self::new(self.repo().clone(), id));
        if let Some(ancestor) = descendants_of.as_ref() {
            // If the the start commit is not descendant of the argument exit early.
            match ancestor.is_ancestor_of(self.id()).await {
                Ok(false) => return stream::empty().boxed(),
                Err(e) => return stream::once(async { Err(e) }).boxed(),
                _ => {}
            }
        }

        let exclude_changeset = opts
            .exclude_changeset_and_ancestors
            .map(|id| Self::new(self.repo().clone(), id));
        if let Some(exclude_changeset) = exclude_changeset.as_ref() {
            // If the the start is ancestor of the argument exit early.
            match self.is_ancestor_of(exclude_changeset.id()).await {
                Ok(true) => return stream::empty().boxed(),
                Err(e) => return stream::once(async { Err(e) }).boxed(),
                _ => {}
            }
        }

        let cs_info_enabled = self.repo.derive_changeset_info_enabled();

        // Helper allowing us to terminate walk when we reach `until_timestamp`.
        let terminate = opts.until_timestamp.map(move |until_timestamp| {
            move |changeset_id| async move {
                let info = if cs_info_enabled {
                    ChangesetInfo::derive(self.ctx(), self.repo().blob_repo(), changeset_id).await?
                } else {
                    let bonsai = changeset_id
                        .load(self.ctx(), self.repo().blob_repo().blobstore())
                        .await?;
                    ChangesetInfo::new(changeset_id, bonsai)
                };
                let date = info.author_date().as_chrono().clone();
                Ok::<_, MononokeError>(date.timestamp() < until_timestamp)
            }
        });

        stream::try_unfold(
            // starting state
            (hashset! { self.id() }, VecDeque::from(vec![self.id()])),
            // unfold
            move |(mut visited, mut queue)| {
                cloned!(descendants_of, exclude_changeset);
                async move {
                    if let Some(changeset_id) = queue.pop_front() {
                        // Terminate in three cases.  The order is important:
                        // cases that do not yield the current changeset must
                        // come first.
                        //
                        // 1. When `until_timestamp` is reached
                        if let Some(terminate) = terminate {
                            if terminate(changeset_id).await? {
                                return Ok(Some((None, (visited, queue))));
                            }
                        }
                        // 2. When we reach the `exclude_changeset_and_ancestors`
                        if let Some(ancestor) = exclude_changeset.as_ref() {
                            if changeset_id == ancestor.id() {
                                return Ok(Some((None, (visited, queue))));
                            }
                        }
                        // 3. When we reach the `descendants_of` ancestor.
                        //    This case includes the changeset.
                        if let Some(ancestor) = descendants_of.as_ref() {
                            if changeset_id == ancestor.id() {
                                return Ok(Some((Some(changeset_id), (visited, queue))));
                            }
                        }
                        let mut parents = self
                            .repo()
                            .blob_repo()
                            .get_changeset_parents_by_bonsai(self.ctx().clone(), changeset_id)
                            .await?;
                        if parents.len() > 1 {
                            if let Some(ancestor) = descendants_of.as_ref() {
                                // In case of merge, find out which branches are worth traversing by
                                // doing ancestry check.
                                parents =
                                    try_join_all(parents.into_iter().map(|parent| async move {
                                        Ok::<_, MononokeError>((
                                            parent,
                                            ancestor.is_ancestor_of(parent).await?,
                                        ))
                                    }))
                                    .await?
                                    .into_iter()
                                    .filter_map(|(parent, ancestry)| ancestry.then_some(parent))
                                    .collect();
                            }
                        }
                        queue.extend(parents.into_iter().filter(|parent| visited.insert(*parent)));
                        Ok(Some((Some(changeset_id), (visited, queue))))
                    } else {
                        Ok::<_, MononokeError>(None)
                    }
                }
            },
        )
        .try_filter_map(move |changeset_id| {
            let changeset = changeset_id
                .map(|changeset_id| ChangesetContext::new(self.repo().clone(), changeset_id));
            async move { Ok::<_, MononokeError>(changeset) }
        })
        .boxed()
    }

    pub async fn diff_root_unordered(
        &self,
        path_restrictions: Option<Vec<MononokePath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
    ) -> Result<Vec<ChangesetPathDiffContext>, MononokeError> {
        self.diff_root(
            path_restrictions,
            diff_items,
            ChangesetFileOrdering::Unordered,
            None,
        )
        .await
    }

    /// Returns additions introduced by the root commit, a.k.a the initial commit
    ///
    /// `self` is considered the "root/initial/genesis" changeset
    /// `path_restrictions` if present will narrow down the diff to given paths
    /// `diff_items` what to include in the output (files, dirs or both)
    pub async fn diff_root(
        &self,
        path_restrictions: Option<Vec<MononokePath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
        ordering: ChangesetFileOrdering,
        limit: Option<usize>,
    ) -> Result<Vec<ChangesetPathDiffContext>, MononokeError> {
        let diff_files = diff_items.contains(&ChangesetDiffItem::FILES);
        let diff_trees = diff_items.contains(&ChangesetDiffItem::TREES);

        self.find_entries(to_vec1(path_restrictions), ordering)
            .await?
            .try_filter_map(|(path, entry)| async move {
                match (path, entry) {
                    (Some(mpath), ManifestEntry::Leaf(_)) if diff_files => Ok(Some(mpath)),
                    (Some(mpath), ManifestEntry::Tree(_)) if diff_trees => Ok(Some(mpath)),
                    _ => Ok(None),
                }
            })
            .map_ok(|mpath| MononokePath::new(Some(mpath)))
            .map_err(MononokeError::from)
            .take(limit.unwrap_or(usize::MAX))
            .and_then(|mp| async move {
                Ok(ChangesetPathDiffContext::Added(
                    ChangesetPathContentContext::new(self.clone(), mp).await?,
                ))
            })
            .try_collect::<Vec<_>>()
            .await
    }

    pub async fn run_hooks(
        &self,
        bookmark: impl AsRef<str>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<Vec<HookOutcome>, MononokeError> {
        Ok(self
            .repo()
            .hook_manager()
            .run_hooks_for_bookmark(
                self.ctx(),
                vec![self.bonsai_changeset().await?].iter(),
                &BookmarkName::new(bookmark.as_ref())?,
                pushvars,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?)
    }
}
