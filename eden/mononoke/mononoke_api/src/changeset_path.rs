/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::identity;
use std::fmt;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use async_recursion::async_recursion;
use async_trait::async_trait;
use blame::fetch_blame_compat;
use blame::fetch_content_for_blame;
use blame::BlameError;
use blame::CompatBlame;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use derived_data::BonsaiDerived;
use fastlog::list_file_history;
use fastlog::CsAndPath;
use fastlog::FastlogError;
use fastlog::FollowMutableFileHistory;
use fastlog::HistoryAcrossDeletions;
use fastlog::TraversalOrder;
use fastlog::Visitor;
use filestore::FetchKey;
use futures::future::try_join_all;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures_lazy_shared::LazyShared;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::Generation;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use reachabilityindex::ReachabilityIndex;
use skiplist::SkiplistIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use xdiff;

pub use xdiff::CopyInfo;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileContext;
use crate::path::MononokePath;
use crate::repo::RepoContext;
use crate::tree::TreeContext;

pub struct HistoryEntry {
    pub name: String,
    pub changeset_id: ChangesetId,
}

#[derive(Default)]
pub struct ChangesetPathHistoryOptions {
    pub until_timestamp: Option<i64>,
    pub descendants_of: Option<ChangesetId>,
    pub exclude_changeset_and_ancestors: Option<ChangesetId>,
    pub follow_history_across_deletions: bool,
    pub follow_mutable_file_history: bool,
}

pub enum PathEntry {
    NotPresent,
    Tree(TreeContext),
    File(FileContext, FileType),
}

/// A diff between two files in extended unified diff format
pub struct UnifiedDiff {
    /// Raw diff as bytes.
    pub raw_diff: Vec<u8>,
    /// One of the diffed files is binary, raw diff contains just a placeholder.
    pub is_binary: bool,
}

type UnodeResult = Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError>;
type FsnodeResult = Result<Option<Entry<FsnodeId, FsnodeFile>>, MononokeError>;
type SkeletonResult = Result<Option<Entry<SkeletonManifestId, ()>>, MononokeError>;
type LinknodeResult = Result<Option<ChangesetId>, MononokeError>;

/// Context that makes it cheap to fetch content info about a path within a changeset.
///
/// A ChangesetPathContentContext may represent a file, a directory, a path where a
/// file or directory has been deleted, or a path where nothing ever existed.
#[derive(Clone)]
pub struct ChangesetPathContentContext {
    changeset: ChangesetContext,
    path: MononokePath,
    fsnode_id: LazyShared<FsnodeResult>,
}

impl fmt::Debug for ChangesetPathContentContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathContentContext(repo={:?} id={:?} path={:?})",
            self.repo().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

/// Context that makes it cheap to fetch history info about a path within a changeset.
pub struct ChangesetPathHistoryContext {
    changeset: ChangesetContext,
    path: MononokePath,
    unode_id: LazyShared<UnodeResult>,
    linknode: LazyShared<LinknodeResult>,
}

impl fmt::Debug for ChangesetPathHistoryContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathHistoryContext(repo={:?} id={:?} path={:?})",
            self.repo().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

/// Context to check if a file or a directory exists in a changeset
pub struct ChangesetPathContext {
    changeset: ChangesetContext,
    path: MononokePath,
    skeleton_manifest_id: LazyShared<SkeletonResult>,
}

impl fmt::Debug for ChangesetPathContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathContext(repo={:?} id={:?} path={:?})",
            self.repo().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

impl ChangesetPathContentContext {
    pub(crate) fn new(changeset: ChangesetContext, path: impl Into<MononokePath>) -> Self {
        Self {
            changeset,
            path: path.into(),
            fsnode_id: LazyShared::new_empty(),
        }
    }

    pub(crate) fn new_with_fsnode_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        fsnode_entry: Entry<FsnodeId, FsnodeFile>,
    ) -> Self {
        Self {
            changeset,
            path: path.into(),
            fsnode_id: LazyShared::new_ready(Ok(Some(fsnode_entry))),
        }
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        &self.changeset.repo()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MononokePath {
        &self.path
    }

    async fn fsnode_id(&self) -> Result<Option<Entry<FsnodeId, FsnodeFile>>, MononokeError> {
        self.fsnode_id
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx().clone();
                    let blobstore = changeset.repo().blob_repo().get_blobstore();
                    let root_fsnode_id = changeset.root_fsnode_id().await?;
                    if let Some(mpath) = path.into() {
                        root_fsnode_id
                            .fsnode_id()
                            .find_entry(ctx, blobstore, Some(mpath))
                            .await
                            .map_err(MononokeError::from)
                    } else {
                        Ok(Some(Entry::Tree(root_fsnode_id.fsnode_id().clone())))
                    }
                }
            })
            .await
    }

    /// Returns `true` if the path exists (as a file or directory) in this commit.
    pub async fn exists(&self) -> Result<bool, MononokeError> {
        // The path exists if there is any kind of fsnode.
        Ok(self.fsnode_id().await?.is_some())
    }

    pub async fn is_file(&self) -> Result<bool, MononokeError> {
        let is_file = match self.fsnode_id().await? {
            Some(Entry::Leaf(_)) => true,
            _ => false,
        };
        Ok(is_file)
    }

    pub async fn is_tree(&self) -> Result<bool, MononokeError> {
        let is_tree = match self.fsnode_id().await? {
            Some(Entry::Tree(_)) => true,
            _ => false,
        };
        Ok(is_tree)
    }

    pub async fn file_type(&self) -> Result<Option<FileType>, MononokeError> {
        let file_type = match self.fsnode_id().await? {
            Some(Entry::Leaf(file)) => Some(*file.file_type()),
            _ => None,
        };
        Ok(file_type)
    }

    /// Returns a `TreeContext` for the tree at this path.  Returns `None` if the path
    /// is not a directory in this commit.
    pub async fn tree(&self) -> Result<Option<TreeContext>, MononokeError> {
        let tree = match self.fsnode_id().await? {
            Some(Entry::Tree(fsnode_id)) => Some(TreeContext::new(self.repo().clone(), fsnode_id)),
            _ => None,
        };
        Ok(tree)
    }

    /// Returns a `FileContext` for the file at this path.  Returns `None` if the path
    /// is not a file in this commit.
    pub async fn file(&self) -> Result<Option<FileContext>, MononokeError> {
        let file = match self.fsnode_id().await? {
            Some(Entry::Leaf(file)) => Some(FileContext::new(
                self.repo().clone(),
                FetchKey::Canonical(*file.content_id()),
            )),
            _ => None,
        };
        Ok(file)
    }

    /// Returns a `TreeContext` or `FileContext` and `FileType` for the tree
    /// or file at this path. Returns `NotPresent` if the path is not a file
    /// or directory in this commit.
    pub async fn entry(&self) -> Result<PathEntry, MononokeError> {
        let entry = match self.fsnode_id().await? {
            Some(Entry::Tree(fsnode_id)) => {
                PathEntry::Tree(TreeContext::new(self.repo().clone(), fsnode_id))
            }
            Some(Entry::Leaf(file)) => PathEntry::File(
                FileContext::new(self.repo().clone(), FetchKey::Canonical(*file.content_id())),
                *file.file_type(),
            ),
            _ => PathEntry::NotPresent,
        };
        Ok(entry)
    }
}

impl ChangesetPathHistoryContext {
    pub(crate) fn new(changeset: ChangesetContext, path: impl Into<MononokePath>) -> Self {
        Self {
            changeset,
            path: path.into(),
            unode_id: LazyShared::new_empty(),
            linknode: LazyShared::new_empty(),
        }
    }

    pub(crate) fn new_with_unode_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Self {
        Self {
            changeset,
            path: path.into(),
            unode_id: LazyShared::new_ready(Ok(Some(unode_entry))),
            linknode: LazyShared::new_empty(),
        }
    }

    pub(crate) fn new_with_deleted_manifest<Manifest: DeletedManifestCommon>(
        changeset: ChangesetContext,
        path: MononokePath,
        deleted_manifest_id: Manifest::Id,
    ) -> Self {
        let ctx = changeset.ctx().clone();
        let blobstore = changeset.repo().blob_repo().blobstore().clone();
        Self {
            changeset,
            path,
            unode_id: LazyShared::new_empty(),
            linknode: LazyShared::new_future(async move {
                let deleted_manifest = deleted_manifest_id.load(&ctx, &blobstore).await?;
                Ok(deleted_manifest.linknode().cloned())
            }),
        }
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        &self.changeset.repo()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MononokePath {
        &self.path
    }

    // pub(crate) for testing
    pub(crate) async fn unode_id(
        &self,
    ) -> Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError> {
        self.unode_id
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx().clone();
                    let blobstore = changeset.repo().blob_repo().get_blobstore();
                    let root_unode_manifest_id = changeset.root_unode_manifest_id().await?;
                    if let Some(mpath) = path.into() {
                        root_unode_manifest_id
                            .manifest_unode_id()
                            .find_entry(ctx, blobstore, Some(mpath))
                            .await
                            .map_err(MononokeError::from)
                    } else {
                        Ok(Some(Entry::Tree(
                            root_unode_manifest_id.manifest_unode_id().clone(),
                        )))
                    }
                }
            })
            .await
    }

    async fn linknode_from_id(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        root: impl RootDeletedManifestIdCommon + 'static,
        path: MononokePath,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        let maybe_id = if let Some(mpath) = path.into() {
            root.find_entry(ctx, blobstore, Some(mpath))
                .await
                .map_err(MononokeError::from)?
        } else {
            Some(root.id().clone())
        };
        if let Some(id) = maybe_id {
            let deleted_manifest = id.load(ctx, blobstore).await?;
            Ok(deleted_manifest.linknode().cloned())
        } else {
            Ok(None)
        }
    }

    async fn linknode(&self) -> Result<Option<ChangesetId>, MononokeError> {
        self.linknode
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx();
                    let blobstore = changeset.repo().blob_repo().blobstore();
                    Self::linknode_from_id(
                        ctx,
                        blobstore,
                        changeset.root_deleted_manifest_v2_id().await?,
                        path,
                    )
                    .await
                }
            })
            .await
    }

    /// Returns the last commit that modified this path.  If there is nothing
    /// at this path, returns `None`.
    pub async fn last_modified(&self) -> Result<Option<ChangesetContext>, MononokeError> {
        match self.unode_id().await? {
            Some(Entry::Tree(manifest_unode_id)) => {
                let ctx = self.changeset.ctx();
                let repo = self.changeset.repo().blob_repo();
                let manifest_unode = manifest_unode_id.load(ctx, repo.blobstore()).await?;
                let cs_id = manifest_unode.linknode().clone();
                Ok(Some(ChangesetContext::new(self.repo().clone(), cs_id)))
            }
            Some(Entry::Leaf(file_unode_id)) => {
                let ctx = self.changeset.ctx();
                let repo = self.changeset.repo().blob_repo();
                let file_unode = file_unode_id.load(ctx, repo.blobstore()).await?;
                let cs_id = file_unode.linknode().clone();
                Ok(Some(ChangesetContext::new(self.repo().clone(), cs_id)))
            }
            None => Ok(None),
        }
    }

    /// Returns the last commit that deleted this path.  If something exists
    /// at this path, or nothing ever existed at this path, returns `None`.
    pub async fn last_deleted(&self) -> Result<Option<ChangesetContext>, MononokeError> {
        Ok(self
            .linknode()
            .await?
            .map(|cs_id| ChangesetContext::new(self.repo().clone(), cs_id)))
    }

    #[async_recursion]
    async fn fetch_mutable_blame(
        &self,
        seen: &mut HashSet<ChangesetId>,
    ) -> Result<(CompatBlame, FileUnodeId), MononokeError> {
        let ctx = self.changeset.ctx();
        let repo_ctx = self.changeset.repo();
        let repo = repo_ctx.blob_repo();
        let my_csid = self.changeset.id();
        let mutable_renames = repo_ctx.mutable_renames();
        let path = self.path.as_mpath().ok_or_else(|| {
            MononokeError::InvalidRequest("Blame is not available for directory: `/`".to_string())
        })?;

        if !seen.insert(my_csid) {
            return Err(anyhow!("Infinite loop in mutable blame").into());
        }

        // First case. Fix up blame directly if I have a mutable rename attached
        let my_mutable_rename = mutable_renames
            .get_rename(ctx, my_csid, Some(path.clone()))
            .await?;
        if let Some(rename) = my_mutable_rename {
            // We have a mutable rename, which replaces our p1 and our path.
            // Recurse to fetch a fully mutated blame for the new p1 parent
            // and path.
            //
            // This covers the case where we are a in the immutable history:
            // a
            // |
            // b  e
            // |  |
            // c  d
            // and there is a mutable rename saying that a's parent should be e, not b.
            // After this, because we did the blame a->e, and we fetched a mutant blame
            // for e, we're guaranteed to be done, even if there are mutations in e's history.
            let src_path = rename
                .src_path()
                .ok_or_else(|| anyhow!("Mutable rename points file to root directory"))?
                .clone();
            let mut src_cs_ctx = repo_ctx
                .changeset(rename.src_cs_id())
                .await?
                .ok_or_else(|| anyhow!("Source changeset of a mutable rename is missing"))?;
            src_cs_ctx.copy_mutable_renames(&self.changeset).await?;
            let src_ctx = src_cs_ctx.path_with_history(src_path.clone())?;
            let (compat_blame, src_content) = src_ctx.blame_with_content(true).await?;
            let src_blame = extract_blame_v2_from_compat(compat_blame)?;

            // Fetch my content, ready to reblame.
            let unode = self
                .unode_id()
                .await?
                .context("Unode missing")?
                .into_leaf()
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!(
                        "Blame is not available for directory: `{}`",
                        self.path
                    ))
                })?;
            let my_content = fetch_content_for_blame(ctx, repo, unode)
                .await?
                .into_bytes()
                .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?;

            // And reblame directly against the parent mutable renames gave us.
            let blame_parent = BlameParent::new(0, src_path, src_content, src_blame);
            let blame = BlameV2::new(my_csid, path.clone(), my_content, vec![blame_parent])?;
            return Ok((CompatBlame::V2(blame), unode));
        }

        // Second case. We don't have a mutable rename attached, so we're going to look
        // at the set of mutable renames for this path, and if any of those renames are ancestors
        // of this commit, we'll apply a mutated blame via BlameV2::apply_mutable_blame to
        // get the final blame result.

        // Check for historic mutable renames - those attached to commits older than us.
        // Given our history graph:
        // a
        // |
        // b
        // |
        // c
        // |\
        // d e
        // where we are b, this looks to see any if c, d, e (etc) has a mutable rename attached to
        // it that affects our current path.
        //
        // We then filter down to remove mutable renames that are ancestors of the currently handled
        // mutable rename, since recursing to get blame will fix those. We can then apply mutation
        // for each blame in any order, because the mutated blame will only affect one ancestry path.
        //
        // For example, if c has a mutable rename for our path, then we do not want to consider mutable
        // renames attached to d or e; however, if c does not, but d and e do, then we want to consider
        // the mutable renames for both d and e.
        let mutable_csids = mutable_renames
            .get_cs_ids_with_rename(ctx, Some(path.clone()))
            .await?;
        let skiplist_index = repo_ctx.skiplist_index();
        let mut possible_mutable_ancestors: Vec<(Generation, ChangesetId)> =
            stream::iter(mutable_csids.into_iter().map(anyhow::Ok))
                .try_filter_map({
                    move |mutated_at| async move {
                        // First, we filter out csids that cannot be reached from here. These
                        // are attached to mutable renames that are either descendants of us, or
                        // in a completely unrelated tree of history.
                        if skiplist_index
                            .query_reachability(ctx, repo.changeset_fetcher(), my_csid, mutated_at)
                            .await?
                        {
                            // We also want to grab generation here, because we're going to sort
                            // by generation and consider "most recent" candidate first
                            let cs_ctx =
                                repo_ctx.changeset(mutated_at).await?.ok_or_else(|| {
                                    anyhow!("Source changeset of a mutable rename is missing")
                                })?;

                            let cs_gen = cs_ctx.generation().await?;
                            Ok(Some((cs_gen, mutated_at)))
                        } else {
                            anyhow::Ok(None)
                        }
                    }
                })
                .try_collect()
                .await?;

        // And turn the list of possible mutable ancestors into a stack sorted by generation
        possible_mutable_ancestors.sort_unstable_by_key(|(gen, _)| *gen);
        // Fetch the immutable blame, which we're going to mutate
        let (blame, unode) = self.blame_impl(false).await?;
        let mut my_blame = extract_blame_v2_from_compat(blame)?;

        // We now have a stack of possible mutable ancestors, sorted so that the highest generation
        // is last. We now pop the last entry from the stack (highest generation) and apply mutation
        // based on that entry. Once that's done, we remove all ancestors of the popped entry
        // from the stack, so that we don't attempt to double-apply a mutation.
        //
        // This will mutate our blame to have all appropriate mutations from ancestors applied
        // If we have mutable blame down two ancestors of a merge, we'd expect that the order
        // of applying those mutations will not affect the final result
        while let Some((_, mutated_csid)) = possible_mutable_ancestors.pop() {
            // Apply mutation for mutated_csid
            let mut mutated_cs_ctx = repo_ctx
                .changeset(mutated_csid)
                .await?
                .ok_or_else(|| anyhow!("Source changeset of a mutable rename is missing"))?;
            mutated_cs_ctx.copy_mutable_renames(&self.changeset).await?;
            let mutated_path_ctx = mutated_cs_ctx.path_with_history(path.clone())?;
            let ((mutated_blame, _), (original_blame, _)) = try_join!(
                mutated_path_ctx.fetch_mutable_blame(seen),
                mutated_path_ctx.blame_impl(false)
            )?;
            let original_blame = extract_blame_v2_from_compat(original_blame)?;
            let mutated_blame = extract_blame_v2_from_compat(mutated_blame)?;
            my_blame.apply_mutable_change(&original_blame, &mutated_blame)?;

            // Rebuild possible_mutable_ancestors without anything that's an ancestor
            // of mutated_csid. This must preserve order, so that we deal with the most
            // recent mutation entries first (which may well remove older mutation entries
            // from the stack)
            possible_mutable_ancestors =
                stream::iter(possible_mutable_ancestors.into_iter().map(anyhow::Ok))
                    .try_filter_map({
                        move |(gen, csid)| async move {
                            if skiplist_index
                                .query_reachability(
                                    ctx,
                                    repo.changeset_fetcher(),
                                    mutated_csid,
                                    csid,
                                )
                                .await?
                            {
                                anyhow::Ok(None)
                            } else {
                                Ok(Some((gen, csid)))
                            }
                        }
                    })
                    .try_collect()
                    .await?;
        }

        Ok((CompatBlame::V2(my_blame), unode))
    }

    async fn fetch_immutable_blame(&self) -> Result<(CompatBlame, FileUnodeId), MononokeError> {
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo().blob_repo();
        let csid = self.changeset.id();
        let mpath = self.path.as_mpath().ok_or_else(|| {
            MononokeError::InvalidRequest("Blame is not available for directory: `/`".to_string())
        })?;

        let (blame, unode) = fetch_blame_compat(ctx, repo, csid, mpath.clone())
            .map_err(|error| match error {
                BlameError::NoSuchPath(_)
                | BlameError::IsDirectory(_)
                | BlameError::Rejected(_) => MononokeError::InvalidRequest(error.to_string()),
                BlameError::DeriveError(e) => MononokeError::from(e),
                _ => MononokeError::from(Error::from(error)),
            })
            .await?;

        Ok((blame, unode))
    }

    async fn blame_impl(
        &self,
        follow_mutable_file_history: bool,
    ) -> Result<(CompatBlame, FileUnodeId), MononokeError> {
        if follow_mutable_file_history {
            self.fetch_mutable_blame(&mut HashSet::new()).await
        } else {
            self.fetch_immutable_blame().await
        }
    }

    /// Blame metadata for this path.
    pub async fn blame(
        &self,
        follow_mutable_file_history: bool,
    ) -> Result<CompatBlame, MononokeError> {
        let (blame, _) = self.blame_impl(follow_mutable_file_history).await?;
        Ok(blame)
    }

    /// Blame metadata for this path, and the content that was blamed.
    pub async fn blame_with_content(
        &self,
        follow_mutable_file_history: bool,
    ) -> Result<(CompatBlame, Bytes), MononokeError> {
        let (blame, file_unode_id) = self.blame_impl(follow_mutable_file_history).await?;
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo().blob_repo();
        let content = fetch_content_for_blame(ctx, repo, file_unode_id)
            .await?
            .into_bytes()
            .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?;
        Ok((blame, content))
    }

    /// Returns a list of `ChangesetContext` for the file at this path that represents
    /// a history of the path.
    pub async fn history(
        &self,
        opts: ChangesetPathHistoryOptions,
    ) -> Result<impl Stream<Item = Result<ChangesetContext, MononokeError>> + '_, MononokeError>
    {
        let ctx = self.changeset.ctx().clone();
        let repo = self.repo().blob_repo().clone();
        let mpath = self.path.as_mpath();

        let descendants_of = match opts.descendants_of {
            Some(descendants_of) => Some((
                descendants_of,
                repo.get_changeset_fetcher()
                    .get_generation_number(ctx.clone(), descendants_of)
                    .await?,
            )),
            None => None,
        };

        let exclude_changeset_and_ancestors = match opts.exclude_changeset_and_ancestors {
            Some(exclude_changeset_and_ancestors) => Some((
                exclude_changeset_and_ancestors,
                repo.get_changeset_fetcher()
                    .get_generation_number(ctx.clone(), exclude_changeset_and_ancestors)
                    .await?,
            )),
            None => None,
        };

        struct FilterVisitor {
            cs_info_enabled: bool,
            until_timestamp: Option<i64>,
            descendants_of: Option<(ChangesetId, Generation)>,
            exclude_changeset_and_ancestors: Option<(ChangesetId, Generation)>,
            cache: HashMap<(Option<CsAndPath>, Vec<CsAndPath>), Vec<CsAndPath>>,
            skiplist_index: Arc<SkiplistIndex>,
        }
        impl FilterVisitor {
            async fn _visit(
                &self,
                ctx: &CoreContext,
                repo: &BlobRepo,
                descendant_cs_id: Option<CsAndPath>,
                mut cs_ids: Vec<CsAndPath>,
            ) -> Result<Vec<CsAndPath>, Error> {
                let cs_info_enabled = self.cs_info_enabled;
                let skiplist_index = self.skiplist_index.clone();
                if let Some(until_ts) = self.until_timestamp {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| async move {
                        let info = if cs_info_enabled {
                            ChangesetInfo::derive(ctx, repo, cs_id).await
                        } else {
                            let bonsai = cs_id.load(&ctx, repo.blobstore()).await?;
                            Ok(ChangesetInfo::new(cs_id, bonsai))
                        }?;
                        let timestamp = info.author_date().as_chrono().timestamp();
                        Ok::<_, Error>((timestamp >= until_ts).then_some((cs_id, path)))
                    }))
                    .await?
                    .into_iter()
                    .filter_map(identity)
                    .collect();
                }
                if let Some((descendants_of, descendants_of_gen)) = self.descendants_of {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| {
                        cloned!(descendant_cs_id, skiplist_index);
                        async move {
                            let changeset_fetcher = repo.get_changeset_fetcher();
                            let cs_gen = changeset_fetcher
                                .get_generation_number(ctx.clone(), cs_id)
                                .await?;
                            if cs_gen < descendants_of_gen {
                                return Ok(None);
                            }
                            let ancestry_check_needed =
                                if let Some((descendant_cs_id, _)) = descendant_cs_id {
                                    let merges = skiplist_index
                                        .find_merges_between(
                                            ctx,
                                            &changeset_fetcher,
                                            cs_id,
                                            descendant_cs_id,
                                        )
                                        .await?;
                                    !merges.is_empty()
                                } else {
                                    true
                                };
                            let mut is_descendant = true;
                            if ancestry_check_needed {
                                is_descendant = skiplist_index
                                    .query_reachability(
                                        ctx,
                                        &repo.get_changeset_fetcher(),
                                        cs_id,
                                        descendants_of,
                                    )
                                    .await?;
                            }
                            Ok::<_, Error>(is_descendant.then_some((cs_id, path)))
                        }
                    }))
                    .await?
                    .into_iter()
                    .filter_map(identity)
                    .collect();
                }
                // Excluding changesest and its ancestors needs to terminate the BFS branch that
                // passes over the changeeset - but not neccesarily visits it because the changeset
                // doesn't need to be a part of given path history. We can enforce that by checking
                // if any of the passed nodes is ancestor of excluded changeset and terminate the
                // branch at those points.
                // To mininimize the number of ancestry checks (which are O(n)) we only do them
                // when the tree traversal goes from a node with generation larger than excluded
                // changeset to generation lower of equal - as only then we have a change of
                // "passing" such changeset.
                if let Some((
                    exclude_changeset_and_ancestors,
                    exclude_changeset_and_ancestors_gen,
                )) = self.exclude_changeset_and_ancestors
                {
                    let changeset_fetcher = &repo.get_changeset_fetcher();
                    let skiplist_index = &skiplist_index;

                    let descendant_cs_gen = if let Some((descendant_cs_id, _)) = descendant_cs_id {
                        Some(
                            changeset_fetcher
                                .get_generation_number(ctx.clone(), descendant_cs_id)
                                .await?,
                        )
                    } else {
                        None
                    };

                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| {
                        async move {
                            let cs_gen = changeset_fetcher
                                .get_generation_number(ctx.clone(), cs_id)
                                .await?;

                            // If the cs_gen is below the cutoff point
                            if cs_gen <= exclude_changeset_and_ancestors_gen {
                                // and the edge if going from above the cutoff.
                                if descendant_cs_gen.is_none()
                                    || descendant_cs_gen
                                        .filter(|gen| gen > &exclude_changeset_and_ancestors_gen)
                                        .is_some()
                                {
                                    // Check the ancestry relationship.
                                    if skiplist_index
                                        .query_reachability(
                                            ctx,
                                            changeset_fetcher,
                                            exclude_changeset_and_ancestors,
                                            cs_id,
                                        )
                                        .await?
                                    {
                                        return Ok::<_, MononokeError>(None);
                                    }
                                }
                            }
                            Ok(Some((cs_id, path)))
                        }
                    }))
                    .await?
                    .into_iter()
                    .filter_map(identity)
                    .collect();
                }
                Ok(cs_ids)
            }
        }
        #[async_trait]
        impl Visitor for FilterVisitor {
            async fn visit(
                &mut self,
                ctx: &CoreContext,
                repo: &BlobRepo,
                descendant_cs_id: Option<CsAndPath>,
                cs_ids: Vec<CsAndPath>,
            ) -> Result<Vec<CsAndPath>, Error> {
                if let Some(res) = self
                    .cache
                    .remove(&(descendant_cs_id.clone(), cs_ids.clone()))
                {
                    Ok(res)
                } else {
                    Ok(self._visit(ctx, repo, descendant_cs_id, cs_ids).await?)
                }
            }

            async fn preprocess(
                &mut self,
                ctx: &CoreContext,
                repo: &BlobRepo,
                descendant_id_cs_ids: Vec<(Option<CsAndPath>, Vec<CsAndPath>)>,
            ) -> Result<(), Error> {
                try_join_all(
                    descendant_id_cs_ids
                        .into_iter()
                        .map(|(descendant_cs_id, cs_ids)| {
                            self._visit(ctx, repo, descendant_cs_id.clone(), cs_ids.clone())
                                .map_ok(move |res| (((descendant_cs_id, cs_ids), res)))
                        }),
                )
                .await?
                .into_iter()
                .for_each(|(k, v)| {
                    self.cache.insert(k, v);
                });
                Ok(())
            }
        }
        let cs_info_enabled = self.repo().derive_changeset_info_enabled();

        let history_across_deletions = if opts.follow_history_across_deletions {
            HistoryAcrossDeletions::Track
        } else {
            HistoryAcrossDeletions::DontTrack
        };

        let use_gen_num_order = tunables::tunables().get_fastlog_use_gen_num_traversal();
        let traversal_order = if use_gen_num_order {
            TraversalOrder::new_gen_num_order(ctx.clone(), repo.get_changeset_fetcher())
        } else {
            TraversalOrder::new_bfs_order()
        };

        let history = list_file_history(
            ctx,
            repo,
            mpath.cloned(),
            self.changeset.id(),
            FilterVisitor {
                cs_info_enabled,
                until_timestamp: opts.until_timestamp,
                descendants_of,
                exclude_changeset_and_ancestors,
                cache: HashMap::new(),
                skiplist_index: self.repo().skiplist_index().clone(),
            },
            history_across_deletions,
            if opts.follow_mutable_file_history {
                FollowMutableFileHistory::MutableFileParents
            } else {
                FollowMutableFileHistory::ImmutableCommitParents
            },
            self.repo().mutable_renames().clone(),
            traversal_order,
        )
        .await
        .map_err(|error| match error {
            FastlogError::InternalError(e) => MononokeError::from(anyhow!(e)),
            FastlogError::DeriveError(e) => MononokeError::from(e),
            FastlogError::LoadableError(e) => MononokeError::from(e),
            FastlogError::Error(e) => MononokeError::from(e),
        })?;

        Ok(history
            .map_err(MononokeError::from)
            .map_ok(move |changeset_id| ChangesetContext::new(self.repo().clone(), changeset_id)))
    }
}

impl ChangesetPathContext {
    pub(crate) fn new(changeset: ChangesetContext, path: impl Into<MononokePath>) -> Self {
        Self {
            changeset,
            path: path.into(),
            skeleton_manifest_id: LazyShared::new_empty(),
        }
    }

    pub(crate) fn new_with_skeleton_manifest_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        skeleton_manifest_entry: Entry<SkeletonManifestId, ()>,
    ) -> Self {
        Self {
            changeset,
            path: path.into(),
            skeleton_manifest_id: LazyShared::new_ready(Ok(Some(skeleton_manifest_entry))),
        }
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        &self.changeset.repo()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MononokePath {
        &self.path
    }

    async fn skeleton_manifest_id(
        &self,
    ) -> Result<Option<Entry<SkeletonManifestId, ()>>, MononokeError> {
        self.skeleton_manifest_id
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx().clone();
                    let blobstore = changeset.repo().blob_repo().get_blobstore();
                    let root_skeleton_manifest_id = changeset.root_skeleton_manifest_id().await?;
                    if let Some(mpath) = path.into() {
                        root_skeleton_manifest_id
                            .skeleton_manifest_id()
                            .find_entry(ctx, blobstore, Some(mpath))
                            .await
                            .map_err(MononokeError::from)
                    } else {
                        Ok(Some(Entry::Tree(
                            root_skeleton_manifest_id.skeleton_manifest_id().clone(),
                        )))
                    }
                }
            })
            .await
    }

    /// Returns `true` if the path exists (as a file or directory) in this commit.
    pub async fn exists(&self) -> Result<bool, MononokeError> {
        // The path exists if there is any kind of skeleton manifest entry.
        Ok(self.skeleton_manifest_id().await?.is_some())
    }

    pub async fn is_file(&self) -> Result<bool, MononokeError> {
        let is_file = match self.skeleton_manifest_id().await? {
            Some(Entry::Leaf(_)) => true,
            _ => false,
        };
        Ok(is_file)
    }

    pub async fn is_tree(&self) -> Result<bool, MononokeError> {
        let is_tree = match self.skeleton_manifest_id().await? {
            Some(Entry::Tree(_)) => true,
            _ => false,
        };
        Ok(is_tree)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UnifiedDiffMode {
    Inline,
    /// Content is not fetched - instead a placeholder diff like
    ///
    /// diff --git a/file.txt b/file.txt
    /// Binary file file.txt has changed
    ///
    /// is generated
    OmitContent,
}

/// Renders the diff (in the git diff format) against some other path.
/// Provided with copy_info will render the diff as copy or move as requested.
/// (does not do the copy-tracking on its own)
/// If `omit_content` is set then unified_diff(...) doesn't fetch content, but just
/// generates a placeholder diff that says that files differ.
pub async fn unified_diff(
    // The diff applied to old_path with produce new_path
    old_path: Option<&ChangesetPathContentContext>,
    new_path: Option<&ChangesetPathContentContext>,
    copy_info: CopyInfo,
    context_lines: usize,
    mode: UnifiedDiffMode,
) -> Result<UnifiedDiff, MononokeError> {
    // Helper for getting file information.
    async fn get_file_data(
        path: Option<&ChangesetPathContentContext>,
        mode: UnifiedDiffMode,
    ) -> Result<Option<xdiff::DiffFile<String, Bytes>>, MononokeError> {
        match path {
            Some(path) => {
                if let Some(file_type) = path.file_type().await? {
                    let file = path.file().await?.ok_or_else(|| {
                        MononokeError::from(Error::msg("assertion error: file should exist"))
                    })?;
                    let file_type = match file_type {
                        FileType::Regular => xdiff::FileType::Regular,
                        FileType::Executable => xdiff::FileType::Executable,
                        FileType::Symlink => xdiff::FileType::Symlink,
                    };
                    let contents = match mode {
                        UnifiedDiffMode::Inline => {
                            let contents = file.content_concat().await?;
                            xdiff::FileContent::Inline(contents)
                        }
                        UnifiedDiffMode::OmitContent => {
                            let content_id = file.metadata().await?.content_id;
                            xdiff::FileContent::Omitted {
                                content_hash: format!("{}", content_id),
                            }
                        }
                    };
                    Ok(Some(xdiff::DiffFile {
                        path: path.path().to_string(),
                        contents,
                        file_type,
                    }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    let (old_diff_file, new_diff_file) =
        try_join!(get_file_data(old_path, mode), get_file_data(new_path, mode))?;
    let is_binary = xdiff::file_is_binary(&old_diff_file) || xdiff::file_is_binary(&new_diff_file);
    let copy_info = match copy_info {
        CopyInfo::None => xdiff::CopyInfo::None,
        CopyInfo::Move => xdiff::CopyInfo::Move,
        CopyInfo::Copy => xdiff::CopyInfo::Copy,
    };
    let opts = xdiff::DiffOpts {
        context: context_lines,
        copy_info,
    };
    let raw_diff = xdiff::diff_unified(old_diff_file, new_diff_file, opts);
    Ok(UnifiedDiff {
        raw_diff,
        is_binary,
    })
}

fn extract_blame_v2_from_compat(blame: CompatBlame) -> Result<BlameV2, Error> {
    if let CompatBlame::V2(blame) = blame {
        Ok(blame)
    } else {
        bail!("Mutable blame only works with blame V2. Ask Source Control oncall for help")
    }
}
