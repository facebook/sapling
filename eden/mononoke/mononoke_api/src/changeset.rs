/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Display;
use std::future::Future;

use anyhow::anyhow;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
use bookmarks::BookmarkKey;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use chrono::DateTime;
use chrono::FixedOffset;
use cloned::cloned;
use commit_graph::AncestorsStreamBuilder;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use commit_graph::LinearAncestorsStreamBuilder;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable;
use fsnodes::RootFsnodeId;
use futures::future::try_join;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_lazy_shared::LazyShared;
use git_types::MappedGitCommitId;
use hooks::CrossRepoPushSource;
use hooks::HookOutcome;
use hooks::PushAuthoredBy;
use manifest::Diff as ManifestDiff;
use manifest::Entry as ManifestEntry;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;
use mercurial_types::Globalrev;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
pub use mononoke_types::Generation;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;
use mononoke_types::Svnrev;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;
use unodes::RootUnodeManifestId;
use vec1::Vec1;

use crate::changeset_path::ChangesetPathContentContext;
use crate::changeset_path::ChangesetPathContext;
use crate::changeset_path::ChangesetPathHistoryContext;
use crate::changeset_path_diff::ChangesetPathDiffContext;
use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::specifiers::ChangesetId;
use crate::specifiers::GitSha1;
use crate::specifiers::HgChangesetId;
use crate::MononokeRepo;

mod find_files;

#[derive(Clone, Debug)]
enum PathMutableHistory {
    /// Checking the mutable history datastore shows no changes
    NoChange,
    /// Change of parent and path
    PathAndChangeset(ChangesetId, MPath),
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
    fn get_copy_from(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::NoChange => None,
            Self::PathAndChangeset(cs_id, path) => Some((*cs_id, path)),
        }
    }
}

#[derive(Clone)]
pub struct ChangesetContext<R> {
    repo_ctx: RepoContext<R>,
    id: ChangesetId,
    bonsai_changeset: LazyShared<Result<BonsaiChangeset, MononokeError>>,
    changeset_info: LazyShared<Result<ChangesetInfo, MononokeError>>,
    root_unode_manifest_id: LazyShared<Result<RootUnodeManifestId, MononokeError>>,
    root_fsnode_id: LazyShared<Result<RootFsnodeId, MononokeError>>,
    root_skeleton_manifest_id: LazyShared<Result<RootSkeletonManifestId, MononokeError>>,
    root_skeleton_manifest_v2_id: LazyShared<Result<RootSkeletonManifestV2Id, MononokeError>>,
    root_deleted_manifest_v2_id: LazyShared<Result<RootDeletedManifestV2Id, MononokeError>>,
    root_bssm_v3_directory_id: LazyShared<Result<RootBssmV3DirectoryId, MononokeError>>,
    /// None if no mutable history, else map from supplied paths to data fetched
    mutable_history: Option<HashMap<MPath, PathMutableHistory>>,
}

#[derive(Default)]
pub struct ChangesetHistoryOptions {
    pub until_timestamp: Option<i64>,
    pub descendants_of: Option<ChangesetId>,
    pub exclude_changeset_and_ancestors: Option<ChangesetId>,
}

#[derive(Default)]
pub struct ChangesetLinearHistoryOptions {
    pub descendants_of: Option<ChangesetId>,
    pub exclude_changeset_and_ancestors: Option<ChangesetId>,
    pub skip: u64,
}

#[derive(Clone)]
pub enum ChangesetFileOrdering {
    Unordered,
    Ordered { after: Option<MPath> },
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangesetDiffItem {
    TREES,
    FILES,
}

impl<R: MononokeRepo> fmt::Debug for ChangesetContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetContext(repo={:?} id={:?})",
            self.repo_ctx().name(),
            self.id()
        )
    }
}

fn to_vec1<X>(maybe_vec: Option<Vec<X>>) -> Option<Vec1<X>> {
    maybe_vec.and_then(|v| Vec1::try_from_vec(v).ok())
}

/// A context object representing a query to a particular commit in a repo.
impl<R: MononokeRepo> ChangesetContext<R> {
    /// Construct a new `MononokeChangeset`.  The changeset must exist
    /// in the repo.
    pub(crate) fn new(repo_ctx: RepoContext<R>, id: ChangesetId) -> Self {
        let bonsai_changeset = LazyShared::new_empty();
        let changeset_info = LazyShared::new_empty();
        let root_unode_manifest_id = LazyShared::new_empty();
        let root_fsnode_id = LazyShared::new_empty();
        let root_skeleton_manifest_id = LazyShared::new_empty();
        let root_skeleton_manifest_v2_id = LazyShared::new_empty();
        let root_deleted_manifest_v2_id = LazyShared::new_empty();
        let root_bssm_v3_directory_id = LazyShared::new_empty();
        Self {
            repo_ctx,
            id,
            changeset_info,
            bonsai_changeset,
            root_unode_manifest_id,
            root_fsnode_id,
            root_skeleton_manifest_id,
            root_skeleton_manifest_v2_id,
            root_deleted_manifest_v2_id,
            root_bssm_v3_directory_id,
            mutable_history: None,
        }
    }

    /// The context for this query.
    pub fn ctx(&self) -> &CoreContext {
        self.repo_ctx.ctx()
    }

    /// Adds copy information from mutable renames as an override to replace
    /// the Bonsai copy information
    pub async fn add_mutable_renames(
        &mut self,
        paths: impl Iterator<Item = MPath>,
    ) -> Result<(), MononokeError> {
        let mutable_renames = &self.repo_ctx.mutable_renames();
        let ctx = self.ctx();
        let cs_id = self.id;

        let copy_info = stream::iter(paths.map(move |path| async move {
            let maybe_rename_entry = mutable_renames.get_rename(ctx, cs_id, path.clone()).await?;
            let rename = match maybe_rename_entry {
                Some(entry) => {
                    let cs_id = entry.src_cs_id();
                    let path = entry.src_path().clone();
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
    pub fn repo_ctx(&self) -> &RepoContext<R> {
        &self.repo_ctx
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    /// Deconstruct the changeset into RepoContext and ChangesetId.
    pub fn into_repo_ctx_and_id(self) -> (RepoContext<R>, ChangesetId) {
        let Self { repo_ctx, id, .. } = self;
        (repo_ctx, id)
    }

    /// The Mercurial ID for the changeset.
    pub async fn hg_id(&self) -> Result<Option<HgChangesetId>, MononokeError> {
        let mapping = self
            .repo_ctx()
            .repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), self.id)
            .await?;
        Ok(mapping.first().map(|(hg_cs_id, _)| *hg_cs_id))
    }

    /// The Globalrev for the changeset.
    pub async fn globalrev(&self) -> Result<Option<Globalrev>, MononokeError> {
        let mapping = self
            .repo_ctx()
            .repo()
            .bonsai_globalrev_mapping()
            .get_globalrev_from_bonsai(self.ctx(), self.id)
            .await?;
        Ok(mapping.into_iter().next())
    }

    /// The SVN revision number for the changeset.
    pub async fn svnrev(&self) -> Result<Option<Svnrev>, MononokeError> {
        let mapping = self
            .repo_ctx()
            .repo()
            .bonsai_svnrev_mapping()
            .get_svnrev_from_bonsai(self.ctx(), self.id)
            .await?;
        Ok(mapping)
    }

    /// The git Sha1 for the changeset (if available).
    pub async fn git_sha1(&self) -> Result<Option<GitSha1>, MononokeError> {
        let maybe_git_sha1 = self
            .repo_ctx()
            .repo()
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(self.ctx(), self.id)
            .await?;
        if maybe_git_sha1.is_none() && self.repo_ctx().derive_gitcommit_enabled() {
            let mapped_git_commit_id = self.derive::<MappedGitCommitId>().await?;
            return Ok(Some(*mapped_git_commit_id.oid()));
        }
        Ok(maybe_git_sha1)
    }

    /// Derive a derivable data type for this changeset.
    // Desugared async syntax so we can return a future with static lifetime.
    fn derive<Derivable: BonsaiDerivable>(
        &self,
    ) -> impl Future<Output = Result<Derivable, MononokeError>> + Send + 'static {
        let ctx = self.ctx().clone();
        let repo_derived_data = self.repo_ctx.repo().repo_derived_data_arc();
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

    pub(crate) async fn root_bssm_v3_directory_id(
        &self,
    ) -> Result<RootBssmV3DirectoryId, MononokeError> {
        self.root_bssm_v3_directory_id
            .get_or_init(|| self.derive::<RootBssmV3DirectoryId>())
            .await
    }

    pub(crate) async fn root_skeleton_manifest_id(
        &self,
    ) -> Result<RootSkeletonManifestId, MononokeError> {
        self.root_skeleton_manifest_id
            .get_or_init(|| self.derive::<RootSkeletonManifestId>())
            .await
    }

    pub(crate) async fn root_skeleton_manifest_v2_id(
        &self,
    ) -> Result<RootSkeletonManifestV2Id, MononokeError> {
        self.root_skeleton_manifest_v2_id
            .get_or_init(|| self.derive::<RootSkeletonManifestV2Id>())
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
    pub async fn root(&self) -> Result<ChangesetPathContentContext<R>, MononokeError> {
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
    ) -> Result<ChangesetPathContentContext<R>, MononokeError>
    where
        P: TryInto<MPath>,
        P::Error: Display,
    {
        ChangesetPathContentContext::new(
            self.clone(),
            path.try_into()
                .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?,
        )
        .await
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    ///
    /// Returns a path history context, which is a context suitable for
    /// queries about the history of this path.
    pub async fn path_with_history<P>(
        &self,
        path: P,
    ) -> Result<ChangesetPathHistoryContext<R>, MononokeError>
    where
        P: TryInto<MPath>,
        P::Error: Display,
    {
        ChangesetPathHistoryContext::new(
            self.clone(),
            path.try_into()
                .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?,
        )
        .await
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    ///
    /// Returns a path context, which is a context that is only suitable for
    /// queries about the type of item that exists at this path.
    ///
    /// If you need to query the content or history of a path, use
    /// `path_with_content` or `path_with_history` instead.
    pub async fn path<P>(&self, path: P) -> Result<ChangesetPathContext<R>, MononokeError>
    where
        P: TryInto<MPath>,
        P::Error: Display,
    {
        ChangesetPathContext::new(
            self.clone(),
            path.try_into()
                .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?,
        )
        .await
    }

    /// Returns a stream of path history contexts for a set of paths.
    ///
    /// This performs an efficient manifest traversal, and as such returns
    /// contexts only for **paths which exist**.
    pub async fn paths_with_history(
        &self,
        paths: impl Iterator<Item = MPath>,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathHistoryContext<R>, MononokeError>>,
        MononokeError,
    > {
        Ok(self
            .root_unode_manifest_id()
            .await?
            .manifest_unode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo_ctx().repo().repo_blobstore().clone(),
                paths,
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathHistoryContext::new_with_unode_entry(
                        changeset.clone(),
                        mpath,
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
        paths: impl Iterator<Item = MPath>,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathContentContext<R>, MononokeError>>,
        MononokeError,
    > {
        Ok(self
            .root_fsnode_id()
            .await?
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo_ctx().repo().repo_blobstore().clone(),
                paths,
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    cloned!(changeset);
                    async move {
                        ChangesetPathContentContext::new_with_fsnode_entry(
                            changeset.clone(),
                            mpath,
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
        paths: impl Iterator<Item = MPath>,
    ) -> Result<impl Stream<Item = Result<ChangesetPathContext<R>, MononokeError>>, MononokeError>
    {
        Ok(self
            .root_skeleton_manifest_id()
            .await?
            .skeleton_manifest_id()
            .find_entries(
                self.ctx().clone(),
                self.repo_ctx().repo().repo_blobstore().clone(),
                paths,
            )
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathContext::new_with_skeleton_manifest_entry(
                        changeset.clone(),
                        mpath,
                        entry,
                    )
                }
            }))
    }

    fn deleted_paths_impl<Root: RootDeletedManifestIdCommon>(
        &self,
        root: Root,
        paths: impl Iterator<Item = MPath> + 'static,
    ) -> impl Stream<Item = Result<ChangesetPathHistoryContext<R>, MononokeError>> + '_ {
        root.find_entries(self.ctx(), self.repo_ctx().repo().repo_blobstore(), paths)
            .map_err(MononokeError::from)
            .and_then({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathHistoryContext::new_with_deleted_manifest::<Root::Manifest>(
                        changeset.clone(),
                        mpath,
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
        paths: impl Iterator<Item = MPath> + 'static,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathHistoryContext<R>, MononokeError>> + '_,
        MononokeError,
    > {
        Ok(self.deleted_paths_impl(self.root_deleted_manifest_v2_id().await?, paths))
    }

    /// Get the `BonsaiChangeset` information for this changeset.
    async fn bonsai_changeset(&self) -> Result<BonsaiChangeset, MononokeError> {
        self.bonsai_changeset
            .get_or_init(|| {
                let ctx = self.ctx().clone();
                let blobstore = self.repo_ctx.repo().repo_blobstore_arc();
                let id = self.id;
                async move { id.load(&ctx, &blobstore).await.map_err(MononokeError::from) }
            })
            .await
    }

    /// Get the `ChangesetInfo` for this changeset.
    pub async fn changeset_info(&self) -> Result<ChangesetInfo, MononokeError> {
        if self.repo_ctx.derive_changeset_info_enabled() {
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
        self.repo_ctx
            .commit_graph()
            .changeset_generation(self.ctx(), self.id)
            .await
            .map_err(|_| {
                MononokeError::NotAvailable(format!("Generation number missing for {:?}", &self.id))
            })
    }

    /// The linear depth of the given changeset
    pub async fn linear_depth(&self) -> Result<u64, MononokeError> {
        self.repo_ctx
            .commit_graph()
            .changeset_linear_depth(self.ctx(), self.id)
            .await
            .map_err(|_| {
                MononokeError::NotAvailable(format!("Linear depth missing for {:?}", &self.id))
            })
    }

    /// All mercurial commit extras as (name, value) pairs.
    pub async fn hg_extras(&self) -> Result<Vec<(String, Vec<u8>)>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .hg_extra()
            .map(|(name, value)| (name.to_string(), Vec::from(value)))
            .collect())
    }

    pub async fn git_extra_headers(
        &self,
    ) -> Result<Option<Vec<(SmallVec<[u8; 24]>, Bytes)>>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .git_extra_headers()
            .map(|headers| {
                headers
                    .map(|(key, value)| (SmallVec::from(key), Bytes::copy_from_slice(value)))
                    .collect()
            }))
    }

    /// File changes associated with the commit.
    pub async fn file_changes(
        &self,
    ) -> Result<SortedVectorMap<NonRootMPath, FileChange>, MononokeError> {
        let bonsai = self.bonsai_changeset().await?;
        let bonsai = bonsai.into_mut();
        Ok(bonsai.file_changes)
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.  A commit is considered its
    /// own ancestor for the purpose of this call.
    pub async fn is_ancestor_of(&self, other_commit: ChangesetId) -> Result<bool, MononokeError> {
        Ok(self
            .repo_ctx()
            .repo()
            .commit_graph()
            .is_ancestor(self.ctx(), self.id, other_commit)
            .await?)
    }

    /// Returns the lowest common ancestor of two commits.
    ///
    /// In case of ambiguity (can happen with multiple merges of the same branches) returns the
    /// common ancestor with lowest id out of those with highest generation number.
    pub async fn common_base_with(
        &self,
        other_commit: ChangesetId,
    ) -> Result<Option<ChangesetContext<R>>, MononokeError> {
        let lca = self
            .repo_ctx()
            .repo()
            .commit_graph()
            .common_base(self.ctx(), self.id, other_commit)
            .await?;
        Ok(lca.first().map(|id| Self::new(self.repo_ctx.clone(), *id)))
    }

    pub async fn diff_unordered(
        &self,
        other: &ChangesetContext<R>,
        include_copies_renames: bool,
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
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
        other: &ChangesetContext<R>,
        include_copies_renames: bool,
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
        ordering: ChangesetFileOrdering,
        limit: Option<usize>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
        // Helper to that checks if a path is within the givien path restrictions
        fn within_restrictions(path: &MPath, path_restrictions: &Option<Vec<MPath>>) -> bool {
            path_restrictions.as_ref().map_or(true, |i| {
                i.iter()
                    .any(|path_restriction| path.is_related_to(path_restriction))
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
                let to_path: MPath = to_path.clone().into();
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
                            let from_path: MPath = from_path.clone().into();
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
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    copy_path_map.keys().cloned(),
                )
                .try_collect::<HashMap<_, _>>();

            // At the same time, find out whether the destinations of copies
            // already existed in the parent.
            let to_path_exists_in_parent = other_root_fsnode_id
                .fsnode_id()
                .find_entries(
                    self.ctx().clone(),
                    other.repo_ctx().repo().repo_blobstore().clone(),
                    to_paths.into_iter(),
                )
                .map_ok(|(path, _)| path)
                .try_collect::<HashSet<_>>();

            let (from_path_to_mf_entry, to_path_exists_in_parent) =
                try_join(from_path_to_mf_entry, to_path_exists_in_parent).await?;

            // Filter out:
            // - Copies where the to_path already existed in the parent.  These don't show up as
            //   copies in the diff view.
            // - Copies where the from_path didn't exist in the parent.  These are indicative of
            //   an invalid bonsai changeset or mutable rename and can be ignored.
            copy_path_map.retain(|from_path, to_paths| {
                to_paths.retain(|to_path| !to_path_exists_in_parent.contains(to_path));
                !to_paths.is_empty() && from_path_to_mf_entry.contains_key(from_path)
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
                self.repo_ctx().repo().repo_blobstore().clone(),
                copy_path_map.keys().cloned(),
            )
            .map_ok(|(from_path, _)| from_path)
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
                | ManifestDiff::Removed(path, ..) => within_restrictions(path, &path_restrictions),
            }
        };

        let diff = match ordering {
            ChangesetFileOrdering::Unordered => {
                // We start from "other" as manfest.diff() is backwards
                other_manifest_root
                    .fsnode_id()
                    .filtered_diff(
                        self.ctx().clone(),
                        self.repo_ctx().repo().repo_blobstore().clone(),
                        self_manifest_root.fsnode_id().clone(),
                        self.repo_ctx().repo().repo_blobstore().clone(),
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
                        self.repo_ctx().repo().repo_blobstore().clone(),
                        self_manifest_root.fsnode_id().clone(),
                        self.repo_ctx().repo().repo_blobstore().clone(),
                        after,
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
                            #[allow(clippy::if_same_then_else)]
                            if copy_path_map.contains_key(&path) {
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
        prefixes: Option<Vec1<MPath>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<
        impl Stream<Item = Result<(MPath, ManifestEntry<SkeletonManifestId, ()>), anyhow::Error>>,
        MononokeError,
    > {
        let root = self.root_skeleton_manifest_id().await?;
        let prefixes = match prefixes {
            Some(prefixes) => prefixes.into_iter().map(PathOrPrefix::Prefix).collect(),
            None => vec![PathOrPrefix::Prefix(MPath::ROOT)],
        };
        let entries = match ordering {
            ChangesetFileOrdering::Unordered => root
                .skeleton_manifest_id()
                .find_entries(
                    self.ctx().clone(),
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    prefixes,
                )
                .left_stream(),
            ChangesetFileOrdering::Ordered { after } => root
                .skeleton_manifest_id()
                .find_entries_ordered(
                    self.ctx().clone(),
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    prefixes,
                    after,
                )
                .right_stream(),
        };
        Ok(entries)
    }

    async fn find_entries_v2(
        &self,
        prefixes: Option<Vec1<MPath>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<
        impl Stream<Item = Result<(MPath, ManifestEntry<SkeletonManifestV2, ()>), anyhow::Error>>,
        MononokeError,
    > {
        let root = self.root_skeleton_manifest_v2_id().await?;
        let manifest = root
            .inner_id()
            .load(self.ctx(), self.repo_ctx().repo().repo_blobstore())
            .await?;
        let prefixes = match prefixes {
            Some(prefixes) => prefixes.into_iter().map(PathOrPrefix::Prefix).collect(),
            None => vec![PathOrPrefix::Prefix(MPath::ROOT)],
        };
        let entries = match ordering {
            ChangesetFileOrdering::Unordered => manifest
                .find_entries(
                    self.ctx().clone(),
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    prefixes,
                )
                .left_stream(),
            ChangesetFileOrdering::Ordered { after } => manifest
                .find_entries_ordered(
                    self.ctx().clone(),
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    prefixes,
                    after,
                )
                .right_stream(),
        };
        Ok(entries)
    }

    /// Returns a stream of `ChangesetContext` for the history of the repository from this commit.
    pub async fn history(
        &self,
        opts: ChangesetHistoryOptions,
    ) -> Result<BoxStream<'_, Result<ChangesetContext<R>, MononokeError>>, MononokeError> {
        let mut ancestors_stream_builder = AncestorsStreamBuilder::new(
            self.repo_ctx().repo().commit_graph_arc(),
            self.ctx().clone(),
            vec![self.id()],
        );

        if let Some(until_timestamp) = opts.until_timestamp {
            ancestors_stream_builder = ancestors_stream_builder.with({
                let ctx = self.ctx().clone();
                let repo_ctx = self.repo_ctx().clone();
                let cs_info_enabled = repo_ctx.derive_changeset_info_enabled();
                move |cs_id| {
                    cloned!(ctx, repo_ctx, cs_info_enabled);
                    async move {
                        let info = if cs_info_enabled {
                            repo_ctx
                                .repo()
                                .repo_derived_data()
                                .derive::<ChangesetInfo>(&ctx, cs_id)
                                .await?
                        } else {
                            let bonsai = cs_id.load(&ctx, repo_ctx.repo().repo_blobstore()).await?;
                            ChangesetInfo::new(cs_id, bonsai)
                        };
                        let date = info.author_date().as_chrono().clone();
                        Ok(date.timestamp() >= until_timestamp)
                    }
                }
            });
        }

        if let Some(descendants_of) = opts.descendants_of {
            ancestors_stream_builder = ancestors_stream_builder.descendants_of(descendants_of);
        }

        if let Some(exclude_changeset_and_ancestors) = opts.exclude_changeset_and_ancestors {
            ancestors_stream_builder = ancestors_stream_builder
                .exclude_ancestors_of(vec![exclude_changeset_and_ancestors]);
        }

        let cs_ids_stream = ancestors_stream_builder.build().await?;

        Ok(cs_ids_stream
            .map_err(MononokeError::from)
            .and_then(move |cs_id| async move {
                Ok::<_, MononokeError>(ChangesetContext::new(self.repo_ctx().clone(), cs_id))
            })
            .boxed())
    }

    pub async fn linear_history(
        &self,
        opts: ChangesetLinearHistoryOptions,
    ) -> Result<BoxStream<'_, Result<ChangesetContext<R>, MononokeError>>, MononokeError> {
        let mut linear_ancestors_stream_builder = LinearAncestorsStreamBuilder::new(
            self.repo_ctx().repo().commit_graph_arc(),
            self.ctx().clone(),
            self.id(),
        )
        .await?;

        if let Some(exclude_changeset_and_ancestors) = opts.exclude_changeset_and_ancestors {
            linear_ancestors_stream_builder = linear_ancestors_stream_builder
                .exclude_ancestors_of(exclude_changeset_and_ancestors)
                .await?;
        }

        if let Some(descendants_of) = opts.descendants_of {
            linear_ancestors_stream_builder = linear_ancestors_stream_builder
                .descendants_of(descendants_of)
                .await?;
        }

        linear_ancestors_stream_builder = linear_ancestors_stream_builder.skip(opts.skip);

        let cs_ids_stream = linear_ancestors_stream_builder.build().await?;

        Ok(cs_ids_stream
            .map_err(MononokeError::from)
            .and_then(move |cs_id| async move {
                Ok::<_, MononokeError>(ChangesetContext::new(self.repo_ctx().clone(), cs_id))
            })
            .boxed())
    }

    pub async fn diff_root_unordered(
        &self,
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
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
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
        ordering: ChangesetFileOrdering,
        limit: Option<usize>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
        let diff_files = diff_items.contains(&ChangesetDiffItem::FILES);
        let diff_trees = diff_items.contains(&ChangesetDiffItem::TREES);

        self.find_entries(to_vec1(path_restrictions), ordering)
            .await?
            .try_filter_map(|(path, entry)| async move {
                match (path.into_optional_non_root_path(), entry) {
                    (Some(mpath), ManifestEntry::Leaf(_)) if diff_files => Ok(Some(mpath)),
                    (Some(mpath), ManifestEntry::Tree(_)) if diff_trees => Ok(Some(mpath)),
                    _ => Ok(None),
                }
            })
            .map_ok(MPath::from)
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
            .repo_ctx()
            .hook_manager()
            .run_changesets_hooks_for_bookmark(
                self.ctx(),
                vec![self.bonsai_changeset().await?].iter(),
                &BookmarkKey::new(bookmark.as_ref())?,
                pushvars,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?)
    }
}
