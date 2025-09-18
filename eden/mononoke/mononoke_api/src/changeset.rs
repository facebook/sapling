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
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
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
use futures_ext::FbStreamExt;
use futures_lazy_shared::LazyShared;
use futures_watchdog::WatchdogExt;
use git_types::MappedGitCommitId;
use hooks::CrossRepoPushSource;
use hooks::HookOutcome;
use hooks::PushAuthoredBy;
use manifest::Diff as ManifestDiff;
use manifest::Entry as ManifestEntry;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_types::Globalrev;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
pub use mononoke_types::Generation;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;
use mononoke_types::SubtreeChange;
use mononoke_types::Svnrev;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
use slog::warn;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;
use unodes::RootUnodeManifestId;
use vec1::Vec1;
use xdiff::CopyInfo;

use crate::MononokeRepo;
use crate::changeset_path::ChangesetPathContentContext;
use crate::changeset_path::ChangesetPathContext;
use crate::changeset_path::ChangesetPathHistoryContext;
use crate::changeset_path_diff::ChangesetPathDiffContext;
use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::specifiers::ChangesetId;
use crate::specifiers::GitSha1;
use crate::specifiers::HgChangesetId;

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

pub struct DirectoryBranchCluster {
    pub cluster_primary: MPath,
    pub secondaries: Vec<MPath>,
}

impl From<metaconfig_types::DirectoryBranchClusterFixedCluster> for DirectoryBranchCluster {
    fn from(cluster: metaconfig_types::DirectoryBranchClusterFixedCluster) -> Self {
        Self {
            cluster_primary: MPath::from(cluster.cluster_primary),
            secondaries: cluster.secondaries.into_iter().map(MPath::from).collect(),
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
        let maybe_hg_id = self
            .repo_ctx()
            .repo()
            .bonsai_hg_mapping()
            .get_hg_from_bonsai(self.ctx(), self.id)
            .await?;
        if maybe_hg_id.is_none() && self.repo_ctx().derive_hgchangesets_enabled() {
            let mapped_hg_id = self.derive::<MappedHgChangesetId>().await?;
            return Ok(Some(mapped_hg_id.hg_changeset_id()));
        }
        Ok(maybe_hg_id)
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
    ) -> impl Future<Output = Result<Derivable, MononokeError>> + Send + 'static + use<Derivable, R>
    {
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

    /// Query a path within the repository. This could be a file or a
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

    /// Query a path within the repository. This could be a file or a
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

    /// Query a path within the repository. This could be a file or a
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
    pub async fn paths_with_history<T: Iterator<Item = MPath>>(
        &self,
        paths: T,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathHistoryContext<R>, MononokeError>> + use<R, T>,
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
    pub async fn paths_with_content<T: Iterator<Item = MPath>>(
        &self,
        paths: T,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathContentContext<R>, MononokeError>> + use<R, T>,
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
    pub async fn paths<T: Iterator<Item = MPath>>(
        &self,
        paths: T,
    ) -> Result<
        impl Stream<Item = Result<ChangesetPathContext<R>, MononokeError>> + use<R, T>,
        MononokeError,
    > {
        if justknobs::eval(
            "scm/mononoke:changeset_path_context_use_skeleton_manifest_v2",
            None,
            Some(self.repo_ctx().name()),
        )? {
            Ok(self
                .root_skeleton_manifest_v2_id()
                .await?
                .into_inner_id()
                .load(self.ctx(), self.repo_ctx().repo().repo_blobstore())
                .await?
                .find_entries(
                    self.ctx().clone(),
                    self.repo_ctx().repo().repo_blobstore().clone(),
                    paths,
                )
                .map_err(MononokeError::from)
                .and_then({
                    let changeset = self.clone();
                    move |(mpath, entry)| {
                        ChangesetPathContext::new_with_entry(
                            changeset.clone(),
                            mpath,
                            entry.map_tree(|_| ()),
                        )
                    }
                })
                .left_stream())
        } else {
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
                        ChangesetPathContext::new_with_entry(
                            changeset.clone(),
                            mpath,
                            entry.map_tree(|_| ()),
                        )
                    }
                })
                .right_stream())
        }
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
    pub async fn bonsai_changeset(&self) -> Result<BonsaiChangeset, MononokeError> {
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

    pub async fn subtree_change_count(&self) -> Result<usize, MononokeError> {
        Ok(self.changeset_info().await?.subtree_change_count())
    }

    pub async fn subtree_changes(
        &self,
    ) -> Result<SortedVectorMap<MPath, SubtreeChange>, MononokeError> {
        let bonsai = self.bonsai_changeset().await?;
        let bonsai = bonsai.into_mut();
        Ok(bonsai.subtree_changes)
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
            .watched(self.ctx().logger())
            .await?;
        Ok(lca.first().map(|id| Self::new(self.repo_ctx.clone(), *id)))
    }

    pub async fn diff_unordered(
        &self,
        other: &ChangesetContext<R>,
        include_copies_renames: bool,
        include_subtree_copies: bool,
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
        self.diff(
            other,
            include_copies_renames,
            include_subtree_copies,
            path_restrictions,
            diff_items,
            ChangesetFileOrdering::Unordered,
            None,
        )
        .watched(self.ctx().logger())
        .await
    }

    /// If the given path is part of a subtree copy, get the subtree copy source, as well as the replacement path
    /// from the subtree copy destination.
    ///
    /// If the path was not part of a subtree copy, return the changeset and path as-is.
    fn get_subtree_copy_source(
        subtree_copy_sources: &manifest::PathTree<Option<(ChangesetContext<R>, MPath)>>,
        changeset: &ChangesetContext<R>,
        path: &MPath,
    ) -> Result<(ChangesetContext<R>, MPath, Option<MPath>), MononokeError> {
        if let Some((dest_path, Some((source_cs, source_path)))) =
            subtree_copy_sources.get_nearest_parent(path, Option::is_some)
        {
            Ok((
                source_cs.clone(),
                path.reparent(&dest_path, source_path)?,
                Some(path.clone()),
            ))
        } else {
            Ok((changeset.clone(), path.clone(), None))
        }
    }

    /// Returns differences between this changeset and some other changeset.
    ///
    /// `self` is considered the "new" changeset (so files missing there are "Removed")
    /// `other` is considered the "old" changeset (so files missing there are "Added")
    /// `include_copies_renames` and `include_subtree_copies` are only available
    /// when diffing commits with its parent
    /// `path_restrictions` if present will narrow down the diff to given paths
    /// `diff_items` what to include in the output (files, dirs or both)
    pub async fn diff(
        &self,
        other: &ChangesetContext<R>,
        include_copies_renames: bool,
        include_subtree_copies: bool,
        path_restrictions: Option<Vec<MPath>>,
        diff_items: BTreeSet<ChangesetDiffItem>,
        ordering: ChangesetFileOrdering,
        limit: Option<usize>,
    ) -> Result<Vec<ChangesetPathDiffContext<R>>, MononokeError> {
        // Helper to that checks if a path is within the given path restrictions
        fn within_restrictions(path: &MPath, path_restrictions: &Option<Vec<MPath>>) -> bool {
            path_restrictions.as_ref().is_none_or(|i| {
                i.iter()
                    .any(|path_restriction| path.is_related_to(path_restriction))
            })
        }

        // map from from_path to to_paths (there may be multiple copies
        // for each from_path, so this maps to a vector of paths)
        let mut copy_path_map = HashMap::new();
        // map from to_path to from_path
        let mut inv_copy_path_map = HashMap::new();
        let mut manifest_replacements = HashMap::new();
        let mut subtree_copy_sources = manifest::PathTree::default();
        // We can only consider copies or subtree copies when comparing with a parent
        let comparing_against_parent = self.parents().await?.contains(&other.id);
        if comparing_against_parent && include_copies_renames {
            let file_changes = self.file_changes().await?;
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
                let path_is_overridden = self
                    .mutable_history
                    .as_ref()
                    .and_then(|history_map| {
                        history_map
                            .get(&to_path)
                            .map(PathMutableHistory::is_override)
                    })
                    .unwrap_or(false);
                if path_is_overridden {
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
        if comparing_against_parent && include_subtree_copies {
            let subtree_changes = self.subtree_changes().await?;
            for (path, change) in subtree_changes {
                if let Some((from_cs_id, from_path)) = change.copy_or_deep_copy_source() {
                    let from_cs =
                        self.repo_ctx()
                            .changeset(from_cs_id)
                            .await?
                            .ok_or_else(|| {
                                MononokeError::from(anyhow!(
                                    "Subtree copy source {from_cs_id} not found"
                                ))
                            })?;
                    let entry = from_cs
                        .root_fsnode_id()
                        .await?
                        .into_fsnode_id()
                        .find_entry(
                            self.ctx().clone(),
                            self.repo_ctx().repo_blobstore(),
                            from_path.clone(),
                        )
                        .await?
                        .ok_or_else(|| {
                            MononokeError::from(anyhow!(
                                "Invalid subtree copy source: {from_cs_id} does not contain {from_path}"
                            ))
                        })?;
                    subtree_copy_sources.insert(path.clone(), Some((from_cs, from_path.clone())));
                    manifest_replacements.insert(path, entry);
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
                // We start from "other" as manifest.diff() is backwards
                other_manifest_root
                    .fsnode_id()
                    .filtered_diff(
                        self.ctx().clone(),
                        self.repo_ctx().repo().repo_blobstore().clone(),
                        self_manifest_root.fsnode_id().clone(),
                        self.repo_ctx().repo().repo_blobstore().clone(),
                        Some,
                        recurse_pruner,
                        manifest_replacements,
                    )
                    .left_stream()
            }
            ChangesetFileOrdering::Ordered { after } => {
                // We must find the weights of manifest replacements.
                let manifest_replacements = stream::iter(manifest_replacements)
                    .map(|(path, entry)| {
                        Ok(async move {
                            match entry {
                                ManifestEntry::Tree(fsnode_id) => {
                                    let fsnode = fsnode_id
                                        .load(self.ctx(), self.repo_ctx().repo().repo_blobstore())
                                        .await?;
                                    let summary = fsnode.summary();
                                    let weight =
                                        summary.descendant_files_count + summary.child_dirs_count;
                                    anyhow::Ok((
                                        path,
                                        ManifestEntry::Tree((weight as usize, fsnode_id)),
                                    ))
                                }
                                ManifestEntry::Leaf(leaf) => Ok((path, ManifestEntry::Leaf(leaf))),
                            }
                        })
                    })
                    .try_buffered(10)
                    .try_collect()
                    .await?;
                // We start from "other" as manifest.diff() is backwards
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
                        manifest_replacements,
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
                                let copy_info = if copied_paths.contains(from_path)
                                    || copy_path_map
                                        .get(*from_path)
                                        .and_then(|to_paths| to_paths.first())
                                        != Some(&path)
                                {
                                    // If the source still exists in the current
                                    // commit, or this isn't the first place it
                                    // was copied to, it was a copy.
                                    CopyInfo::Copy
                                } else {
                                    // If it doesn't, and this is the first place
                                    // it was copied to, it was a move.
                                    CopyInfo::Move
                                };

                                let from = ChangesetPathContentContext::new_with_fsnode_entry(
                                    other.clone(),
                                    (**from_path).clone(),
                                    *from_entry,
                                )
                                .await?;
                                Some(ChangesetPathDiffContext::new_file(
                                    self.clone(),
                                    path.clone(),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path,
                                            entry,
                                        )
                                        .await?,
                                    ),
                                    Some(from),
                                    copy_info,
                                    None,
                                )?)
                            } else {
                                Some(ChangesetPathDiffContext::new_file(
                                    self.clone(),
                                    path.clone(),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path,
                                            entry,
                                        )
                                        .await?,
                                    ),
                                    None,
                                    CopyInfo::None,
                                    None,
                                )?)
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
                                let (source, source_path, replacement_path) =
                                    Self::get_subtree_copy_source(
                                        &subtree_copy_sources,
                                        other,
                                        &path,
                                    )?;
                                Some(ChangesetPathDiffContext::new_file(
                                    self.clone(),
                                    path.clone(),
                                    None,
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            source,
                                            source_path,
                                            entry,
                                        )
                                        .await?,
                                    ),
                                    CopyInfo::None,
                                    replacement_path,
                                )?)
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
                                let (source, source_path, replacement_path) =
                                    Self::get_subtree_copy_source(
                                        &subtree_copy_sources,
                                        other,
                                        &path,
                                    )?;
                                Some(ChangesetPathDiffContext::new_file(
                                    self.clone(),
                                    path.clone(),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path.clone(),
                                            to_entry,
                                        )
                                        .await?,
                                    ),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            source,
                                            source_path,
                                            from_entry,
                                        )
                                        .await?,
                                    ),
                                    CopyInfo::None,
                                    replacement_path,
                                )?)
                            }
                        }
                        ManifestDiff::Added(path, entry @ ManifestEntry::Tree(_)) => {
                            if !diff_trees || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                Some(ChangesetPathDiffContext::new_tree(
                                    self.clone(),
                                    path.clone(),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path,
                                            entry,
                                        )
                                        .await?,
                                    ),
                                    None,
                                    None,
                                )?)
                            }
                        }
                        ManifestDiff::Removed(path, entry @ ManifestEntry::Tree(_)) => {
                            if !diff_trees || !within_restrictions(&path, &path_restrictions) {
                                None
                            } else {
                                let (source, source_path, replacement_path) =
                                    Self::get_subtree_copy_source(
                                        &subtree_copy_sources,
                                        other,
                                        &path,
                                    )?;
                                Some(ChangesetPathDiffContext::new_tree(
                                    self.clone(),
                                    path.clone(),
                                    None,
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            source,
                                            source_path,
                                            entry,
                                        )
                                        .await?,
                                    ),
                                    replacement_path,
                                )?)
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
                                let (source, source_path, replacement_path) =
                                    Self::get_subtree_copy_source(
                                        &subtree_copy_sources,
                                        other,
                                        &path,
                                    )?;
                                Some(ChangesetPathDiffContext::new_tree(
                                    self.clone(),
                                    path.clone(),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            self.clone(),
                                            path.clone(),
                                            to_entry,
                                        )
                                        .await?,
                                    ),
                                    Some(
                                        ChangesetPathContentContext::new_with_fsnode_entry(
                                            source,
                                            source_path,
                                            from_entry,
                                        )
                                        .await?,
                                    ),
                                    replacement_path,
                                )?)
                            }
                        }
                        // We've already covered all practical possibilities as there are no "changed"
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
        impl Stream<Item = Result<(MPath, ManifestEntry<SkeletonManifestId, ()>), anyhow::Error>>
        + use<R>,
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
        impl Stream<Item = Result<(MPath, ManifestEntry<SkeletonManifestV2, ()>), anyhow::Error>>
        + use<R>,
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
            .watched(self.ctx().logger())
            .await?
            .yield_periodically()
            .on_large_overshoot(|budget, elapsed| {
                warn!(self.ctx().logger(), "yield_periodically(): budget overshot: current_budget={budget:?}, elapsed={elapsed:?}");
            })
            .try_filter_map(|(path, entry)| async move {
                match (path.into_optional_non_root_path(), entry) {
                    (Some(mpath), ManifestEntry::Leaf(_)) if diff_files => Ok(Some((MPath::from(mpath), false))),
                    (Some(mpath), ManifestEntry::Tree(_)) if diff_trees => Ok(Some((MPath::from(mpath), true))),
                    _ => Ok(None),
                }
            })
            .map_err(MononokeError::from)
            .take(limit.unwrap_or(usize::MAX))
            .and_then(|(path, is_tree)| async move {
                let base = ChangesetPathContentContext::new(self.clone(), path.clone()).await?;
                if is_tree {
                    Ok(ChangesetPathDiffContext::new_tree(
                        self.clone(),
                        path,
                        Some(base),
                        None,
                        None,
                    )?)
                } else {
                    Ok(ChangesetPathDiffContext::new_file(
                        self.clone(),
                        path,
                        Some(base),
                        None,
                        CopyInfo::None,
                        None,
                    )?)
                }
            })
            .try_collect::<Vec<_>>()
            .watched(self.ctx().logger())
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
                &[self.bonsai_changeset().await?],
                &BookmarkKey::new(bookmark.as_ref())?,
                pushvars,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?)
    }

    pub async fn directory_branch_clusters(
        &self,
        filter_path_prefixes: Option<Vec<MPath>>,
        after: Option<MPath>,
    ) -> Result<impl Iterator<Item = DirectoryBranchCluster>, MononokeError> {
        let Some(fixed_config) = self
            .repo_ctx()
            .repo()
            .repo_config()
            .directory_branch_cluster_config
            .as_ref()
            .map(|config| &config.fixed_config)
        else {
            return Ok(vec![].into_iter());
        };

        let root_manifest_id = self
            .root_skeleton_manifest_id()
            .await?
            .into_skeleton_manifest_id();

        // Find which paths in the cluster config exist in this commit.  We will
        // filter out any primary or secondary paths that don't exist.
        let cluster_paths: HashSet<MPath> = fixed_config
            .clusters
            .iter()
            .flat_map(|cluster| {
                std::iter::once(&cluster.cluster_primary).chain(cluster.secondaries.iter())
            })
            .cloned()
            .map(MPath::from)
            .collect();
        let existing_paths = root_manifest_id
            .find_entries(
                self.ctx().clone(),
                self.repo_ctx().repo().repo_blobstore().clone(),
                cluster_paths.into_iter().map(PathOrPrefix::Path),
            )
            .map_ok(|(path, _entry)| path)
            .try_collect::<HashSet<_>>()
            .await?;

        let mut clusters = vec![];
        for cluster in fixed_config.clusters.iter() {
            // Apply fixed cluster config to the requested commit.   We need to filter out
            // the parts of the cluster config that don't apply to this commit by removing
            // any paths that don't exist in the commit.

            let mut cluster = DirectoryBranchCluster::from(cluster.clone());

            // Filter out any secondary paths that don't exist.
            cluster
                .secondaries
                .retain(|path| existing_paths.contains(path));

            if !existing_paths.contains(&cluster.cluster_primary) && !cluster.secondaries.is_empty()
            {
                // If the primary path doesn't exist, promote the first secondary (if any) to be the primary.
                cluster.cluster_primary = cluster.secondaries.remove(0);
            }
            // If the cluster no longer has any secondary paths, skip it.
            if cluster.secondaries.is_empty() {
                continue;
            }
            // Now that we know what the (possibly promoted) primary is at this commit, if the
            // cluster primary is before the `after` parameter, skip it.
            if let Some(after) = after.as_ref()
                && &cluster.cluster_primary <= after
            {
                continue;
            }
            // If there are path prefix filters, and none of the paths in the cluster match, skip it.
            if let Some(filter_path_prefixes) = &filter_path_prefixes
                && !filter_path_prefixes.iter().any(|prefix| {
                    prefix.is_prefix_of(&cluster.cluster_primary)
                        || cluster.secondaries.iter().any(|s| prefix.is_prefix_of(s))
                })
            {
                continue;
            }

            clusters.push(cluster);
        }

        // Sort the clusters by primary path.
        clusters.sort_by(|a, b| a.cluster_primary.cmp(&b.cluster_primary));

        Ok(clusters.into_iter())
    }
}
