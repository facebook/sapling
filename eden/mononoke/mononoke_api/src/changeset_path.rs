/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;

use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::Bytes;
use changeset_fetcher::ChangesetFetcherArc;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use derived_data::BonsaiDerived;
use filestore::FetchKey;
use futures::future::try_join_all;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_lazy_shared::LazyShared;
use history_traversal::list_file_history;
use history_traversal::CsAndPath;
use history_traversal::FastlogError;
use history_traversal::FollowMutableRenames;
use history_traversal::HistoryAcrossDeletions;
use history_traversal::TraversalOrder;
use history_traversal::Visitor;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use mononoke_types::ContentMetadataV2;
/// Metadata about a file.
pub use mononoke_types::ContentMetadataV2 as FileMetadata;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use repo_blobstore::RepoBlobstoreRef;

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

#[derive(Default, Clone, Copy)]
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
    pub(crate) async fn new(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            fsnode_id: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_fsnode_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        fsnode_entry: Entry<FsnodeId, FsnodeFile>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            fsnode_id: LazyShared::new_ready(Ok(Some(fsnode_entry))),
        })
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        self.changeset.repo()
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
                    let blobstore = changeset.repo().blob_repo().repo_blobstore().clone();
                    let root_fsnode_id = changeset.root_fsnode_id().await?;
                    if let Some(mpath) = path.into_mpath() {
                        root_fsnode_id
                            .fsnode_id()
                            .find_entry(ctx, blobstore, MPath::from(mpath))
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
            Some(Entry::Tree(fsnode_id)) => {
                Some(TreeContext::new_authorized(self.repo().clone(), fsnode_id))
            }
            _ => None,
        };
        Ok(tree)
    }

    /// Returns a `FileContext` for the file at this path.  Returns `None` if the path
    /// is not a file in this commit.
    pub async fn file(&self) -> Result<Option<FileContext>, MononokeError> {
        let file = match self.fsnode_id().await? {
            Some(Entry::Leaf(file)) => Some(FileContext::new_authorized(
                self.repo().clone(),
                FetchKey::Canonical(*file.content_id()),
            )),
            _ => None,
        };
        Ok(file)
    }

    pub async fn file_content(&self) -> Result<Option<Bytes>, MononokeError> {
        Ok(match self.file().await? {
            Some(context) => Some(context.content_concat().await?),
            None => None,
        })
    }

    pub async fn file_metadata(&self) -> Result<Option<ContentMetadataV2>, MononokeError> {
        let metadata = match self.file().await? {
            Some(context) => Some(context.metadata().await?),
            None => None,
        };
        Ok(metadata)
    }

    /// Returns a `TreeContext` or `FileContext` and `FileType` for the tree
    /// or file at this path. Returns `NotPresent` if the path is not a file
    /// or directory in this commit.
    pub async fn entry(&self) -> Result<PathEntry, MononokeError> {
        let entry = match self.fsnode_id().await? {
            Some(Entry::Tree(fsnode_id)) => {
                PathEntry::Tree(TreeContext::new_authorized(self.repo().clone(), fsnode_id))
            }
            Some(Entry::Leaf(file)) => PathEntry::File(
                FileContext::new_authorized(
                    self.repo().clone(),
                    FetchKey::Canonical(*file.content_id()),
                ),
                *file.file_type(),
            ),
            _ => PathEntry::NotPresent,
        };
        Ok(entry)
    }
}

impl ChangesetPathHistoryContext {
    pub(crate) async fn new(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            unode_id: LazyShared::new_empty(),
            linknode: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_unode_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            unode_id: LazyShared::new_ready(Ok(Some(unode_entry))),
            linknode: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_deleted_manifest<Manifest: DeletedManifestCommon>(
        changeset: ChangesetContext,
        path: MononokePath,
        deleted_manifest_id: Manifest::Id,
    ) -> Result<Self, MononokeError> {
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        let ctx = changeset.ctx().clone();
        let blobstore = changeset.repo().blob_repo().repo_blobstore().clone();
        Ok(Self {
            changeset,
            path,
            unode_id: LazyShared::new_empty(),
            linknode: LazyShared::new_future(async move {
                let deleted_manifest = deleted_manifest_id.load(&ctx, &blobstore).await?;
                Ok(deleted_manifest.linknode().cloned())
            }),
        })
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        self.changeset.repo()
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
                    let blobstore = changeset.repo().blob_repo().repo_blobstore().clone();
                    let root_unode_manifest_id = changeset.root_unode_manifest_id().await?;
                    if let Some(mpath) = path.into_mpath() {
                        root_unode_manifest_id
                            .manifest_unode_id()
                            .find_entry(ctx, blobstore, MPath::from(mpath))
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
        let maybe_id = if let Some(mpath) = path.into_mpath() {
            root.find_entry(ctx, blobstore, MPath::from(mpath))
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
                    let blobstore = changeset.repo().blob_repo().repo_blobstore();
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
                let manifest_unode = manifest_unode_id.load(ctx, repo.repo_blobstore()).await?;
                let cs_id = manifest_unode.linknode().clone();
                Ok(Some(ChangesetContext::new(self.repo().clone(), cs_id)))
            }
            Some(Entry::Leaf(file_unode_id)) => {
                let ctx = self.changeset.ctx();
                let repo = self.changeset.repo().blob_repo();
                let file_unode = file_unode_id.load(ctx, repo.repo_blobstore()).await?;
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

    /// Blame metadata for this path.
    pub async fn blame(&self, follow_mutable_file_history: bool) -> Result<BlameV2, MononokeError> {
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo().inner_repo();
        let csid = self.changeset.id();
        let path = self.path.as_mpath();
        let (blame, _) =
            history_traversal::blame(ctx, repo, csid, path, follow_mutable_file_history).await?;
        Ok(blame)
    }

    /// Blame metadata for this path, and the content that was blamed.
    pub async fn blame_with_content(
        &self,
        follow_mutable_file_history: bool,
    ) -> Result<(BlameV2, Bytes), MononokeError> {
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo().inner_repo();
        let csid = self.changeset.id();
        let path = self.path.as_mpath();
        Ok(history_traversal::blame_with_content(
            ctx,
            repo,
            csid,
            path,
            follow_mutable_file_history,
        )
        .await?)
    }

    /// Returns a list of `ChangesetContext` for the file at this path that represents
    /// a history of the path.
    pub async fn history(
        &self,
        opts: ChangesetPathHistoryOptions,
    ) -> Result<BoxStream<'_, Result<ChangesetContext, MononokeError>>, MononokeError> {
        let repo = self.repo().repo().clone();
        let mpath = self.path.as_mpath();

        if let Some(descendants_of) = opts.descendants_of {
            if !repo
                .commit_graph()
                .is_ancestor(
                    self.changeset().ctx(),
                    descendants_of,
                    self.changeset().id(),
                )
                .await?
            {
                return Ok(stream::empty().boxed());
            }
        }

        struct FilterVisitor {
            cs_info_enabled: bool,
            until_timestamp: Option<i64>,
            descendants_of: Option<ChangesetId>,
            exclude_changeset_and_ancestors: Option<ChangesetId>,
            cache: HashMap<(Option<CsAndPath>, Vec<CsAndPath>), Vec<CsAndPath>>,
        }
        impl FilterVisitor {
            async fn _visit(
                &self,
                ctx: &CoreContext,
                repo: &impl history_traversal::Repo,
                _descendant_cs_id: Option<CsAndPath>,
                mut cs_ids: Vec<CsAndPath>,
            ) -> Result<Vec<CsAndPath>, Error> {
                let cs_info_enabled = self.cs_info_enabled;
                if let Some(until_ts) = self.until_timestamp {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| async move {
                        let info = if cs_info_enabled {
                            ChangesetInfo::derive(ctx, repo, cs_id).await
                        } else {
                            let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
                            Ok(ChangesetInfo::new(cs_id, bonsai))
                        }?;
                        let timestamp = info.author_date().as_chrono().timestamp();
                        Ok::<_, Error>((timestamp >= until_ts).then_some((cs_id, path)))
                    }))
                    .await?
                    .into_iter()
                    .filter_map(std::convert::identity)
                    .collect();
                }

                if let Some(descendants_of) = self.descendants_of {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| async move {
                        if repo
                            .commit_graph()
                            .is_ancestor(ctx, descendants_of, cs_id)
                            .await?
                        {
                            anyhow::Ok(Some((cs_id, path)))
                        } else {
                            anyhow::Ok(None)
                        }
                    }))
                    .await?
                    .into_iter()
                    .filter_map(std::convert::identity)
                    .collect();
                }

                if let Some(exclude_changeset_and_ancestors) = self.exclude_changeset_and_ancestors
                {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|(cs_id, path)| async move {
                        if repo
                            .commit_graph()
                            .is_ancestor(ctx, cs_id, exclude_changeset_and_ancestors)
                            .await?
                        {
                            Ok::<_, MononokeError>(None)
                        } else {
                            Ok::<_, MononokeError>(Some((cs_id, path)))
                        }
                    }))
                    .await?
                    .into_iter()
                    .filter_map(std::convert::identity)
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
                repo: &impl history_traversal::Repo,
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
                repo: &impl history_traversal::Repo,
                descendant_id_cs_ids: Vec<(Option<CsAndPath>, Vec<CsAndPath>)>,
            ) -> Result<(), Error> {
                try_join_all(
                    descendant_id_cs_ids
                        .into_iter()
                        .map(|(descendant_cs_id, cs_ids)| {
                            self._visit(ctx, repo, descendant_cs_id.clone(), cs_ids.clone())
                                .map_ok(move |res| ((descendant_cs_id, cs_ids), res))
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

        let history = list_file_history(
            self.changeset.ctx(),
            self.repo().inner_repo(),
            mpath.cloned().into(),
            self.changeset.id(),
            FilterVisitor {
                cs_info_enabled,
                until_timestamp: opts.until_timestamp,
                descendants_of: opts.descendants_of,
                exclude_changeset_and_ancestors: opts.exclude_changeset_and_ancestors,
                cache: HashMap::new(),
            },
            history_across_deletions,
            if opts.follow_mutable_file_history {
                FollowMutableRenames::Yes
            } else {
                FollowMutableRenames::No
            },
            self.repo().mutable_renames().clone(),
            TraversalOrder::new_gen_num_order(
                self.changeset.ctx().clone(),
                repo.changeset_fetcher_arc(),
            ),
        )
        .await
        .map_err(|error| match error {
            FastlogError::InternalError(e) => MononokeError::from(anyhow!(e)),
            FastlogError::DerivationError(e) => MononokeError::from(e),
            FastlogError::LoadableError(e) => MononokeError::from(e),
            FastlogError::Error(e) => MononokeError::from(e),
        })?;

        Ok(history
            .map_err(MononokeError::from)
            .map_ok(move |changeset_id| ChangesetContext::new(self.repo().clone(), changeset_id))
            .boxed())
    }
}

impl ChangesetPathContext {
    pub(crate) async fn new(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            skeleton_manifest_id: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_skeleton_manifest_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        skeleton_manifest_entry: Entry<SkeletonManifestId, ()>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo().inner_repo(),
                changeset.id(),
                path.as_mpath().into(),
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            skeleton_manifest_id: LazyShared::new_ready(Ok(Some(skeleton_manifest_entry))),
        })
    }

    /// The `RepoContext` for this query.
    pub fn repo(&self) -> &RepoContext {
        self.changeset.repo()
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
                    let blobstore = changeset.repo().blob_repo().repo_blobstore().clone();
                    let root_skeleton_manifest_id = changeset.root_skeleton_manifest_id().await?;
                    if let Some(mpath) = path.into_mpath() {
                        root_skeleton_manifest_id
                            .skeleton_manifest_id()
                            .find_entry(ctx, blobstore, MPath::from(mpath))
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
