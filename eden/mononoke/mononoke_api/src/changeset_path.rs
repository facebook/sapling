/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;

use anyhow::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use filestore::FetchKey;
use futures::future::TryFutureExt;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::stream::FbStreamExt;
use futures_lazy_shared::LazyShared;
use futures_watchdog::WatchdogExt;
use history_traversal::CsAndPath;
use history_traversal::FastlogError;
use history_traversal::FollowMutableRenames;
use history_traversal::HistoryAcrossDeletions;
use history_traversal::TraversalOrder;
use history_traversal::Visitor;
use history_traversal::list_file_history;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use mononoke_types::ContentMetadataV2;
/// Metadata about a file.
pub use mononoke_types::ContentMetadataV2 as FileMetadata;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use repo_blobstore::RepoBlobstoreRef;

use crate::MononokeRepo;
use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileContext;
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

pub enum PathEntry<R> {
    NotPresent,
    Tree(TreeContext<R>),
    File(FileContext<R>, FileType),
}

type UnodeResult = Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError>;
type FsnodeResult = Result<Option<Entry<FsnodeId, FsnodeFile>>, MononokeError>;
type LinknodeResult = Result<Option<ChangesetId>, MononokeError>;

/// Context that makes it cheap to fetch content info about a path within a changeset.
///
/// A ChangesetPathContentContext may represent a file, a directory, a path where a
/// file or directory has been deleted, or a path where nothing ever existed.
#[derive(Clone)]
pub struct ChangesetPathContentContext<R> {
    changeset: ChangesetContext<R>,
    path: MPath,
    fsnode_id: LazyShared<FsnodeResult>,
}

impl<R: MononokeRepo> fmt::Debug for ChangesetPathContentContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathContentContext(repo={:?} id={:?} path={:?})",
            self.repo_ctx().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

/// Context that makes it cheap to fetch history info about a path within a changeset.
pub struct ChangesetPathHistoryContext<R> {
    changeset: ChangesetContext<R>,
    path: MPath,
    unode_id: LazyShared<UnodeResult>,
    linknode: LazyShared<LinknodeResult>,
}

impl<R: MononokeRepo> fmt::Debug for ChangesetPathHistoryContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathHistoryContext(repo={:?} id={:?} path={:?})",
            self.repo_ctx().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

/// Context to check if a file or a directory exists in a changeset
pub struct ChangesetPathContext<R> {
    changeset: ChangesetContext<R>,
    path: MPath,
    entry_kind: LazyShared<Result<Option<Entry<(), ()>>, MononokeError>>,
}

impl<R: MononokeRepo> fmt::Debug for ChangesetPathContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetPathContext(repo={:?} id={:?} path={:?})",
            self.repo_ctx().name(),
            self.changeset().id(),
            self.path()
        )
    }
}

impl<R: MononokeRepo> ChangesetPathContentContext<R> {
    pub(crate) async fn new(
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            fsnode_id: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_fsnode_entry(
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
        fsnode_entry: Entry<FsnodeId, FsnodeFile>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            fsnode_id: LazyShared::new_ready(Ok(Some(fsnode_entry))),
        })
    }

    /// The `RepoContext` for this query.
    pub fn repo_ctx(&self) -> &RepoContext<R> {
        self.changeset.repo_ctx()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext<R> {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MPath {
        &self.path
    }

    async fn fsnode_id(&self) -> Result<Option<Entry<FsnodeId, FsnodeFile>>, MononokeError> {
        self.fsnode_id
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx().clone();
                    let blobstore = changeset.repo_ctx().repo().repo_blobstore().clone();
                    let root_fsnode_id = changeset.root_fsnode_id().await?;
                    if let Some(mpath) = path.into_optional_non_root_path() {
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
    pub async fn tree(&self) -> Result<Option<TreeContext<R>>, MononokeError> {
        let tree = match self.fsnode_id().await? {
            Some(Entry::Tree(fsnode_id)) => Some(TreeContext::new_authorized(
                self.repo_ctx().clone(),
                fsnode_id,
            )),
            _ => None,
        };
        Ok(tree)
    }

    /// Returns a `FileContext` for the file at this path.  Returns `None` if the path
    /// is not a file in this commit.
    pub async fn file(&self) -> Result<Option<FileContext<R>>, MononokeError> {
        let file = match self.fsnode_id().await? {
            Some(Entry::Leaf(file)) => Some(FileContext::new_authorized(
                self.repo_ctx().clone(),
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

    pub async fn file_change(&self) -> Result<Option<FileChange>, MononokeError> {
        let non_root_mpath = NonRootMPath::try_from(self.path.clone())?;
        let file_changes = self.changeset.file_changes().await?;
        let file_change = file_changes.get(&non_root_mpath);
        match file_change {
            Some(file_change) => Ok(Some(file_change.clone())),
            None => Ok(None),
        }
    }

    /// Returns a `TreeContext` or `FileContext` and `FileType` for the tree
    /// or file at this path. Returns `NotPresent` if the path is not a file
    /// or directory in this commit.
    pub async fn entry(&self) -> Result<PathEntry<R>, MononokeError> {
        let entry = match self.fsnode_id().await? {
            Some(Entry::Tree(fsnode_id)) => PathEntry::Tree(TreeContext::new_authorized(
                self.repo_ctx().clone(),
                fsnode_id,
            )),
            Some(Entry::Leaf(file)) => PathEntry::File(
                FileContext::new_authorized(
                    self.repo_ctx().clone(),
                    FetchKey::Canonical(*file.content_id()),
                ),
                *file.file_type(),
            ),
            _ => PathEntry::NotPresent,
        };
        Ok(entry)
    }
}

impl<R: MononokeRepo> ChangesetPathHistoryContext<R> {
    pub(crate) async fn new(
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
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
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
        unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
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
        changeset: ChangesetContext<R>,
        path: MPath,
        deleted_manifest_id: Manifest::Id,
    ) -> Result<Self, MononokeError> {
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
            )
            .await?;
        let ctx = changeset.ctx().clone();
        let blobstore = changeset.repo_ctx().repo().repo_blobstore().clone();
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
    pub fn repo_ctx(&self) -> &RepoContext<R> {
        self.changeset.repo_ctx()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext<R> {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MPath {
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
                    let blobstore = changeset.repo_ctx().repo().repo_blobstore().clone();
                    let root_unode_manifest_id = changeset.root_unode_manifest_id().await?;
                    if let Some(mpath) = path.into_optional_non_root_path() {
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
        path: MPath,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        let maybe_id = if let Some(mpath) = path.into_optional_non_root_path() {
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
                    let blobstore = changeset.repo_ctx().repo().repo_blobstore();
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
    pub async fn last_modified(&self) -> Result<Option<ChangesetContext<R>>, MononokeError> {
        match self.unode_id().await? {
            Some(Entry::Tree(manifest_unode_id)) => {
                let ctx = self.changeset.ctx();
                let repo = self.changeset.repo_ctx().repo();
                let manifest_unode = manifest_unode_id.load(ctx, repo.repo_blobstore()).await?;
                let cs_id = manifest_unode.linknode().clone();
                Ok(Some(ChangesetContext::new(self.repo_ctx().clone(), cs_id)))
            }
            Some(Entry::Leaf(file_unode_id)) => {
                let ctx = self.changeset.ctx();
                let repo = self.changeset.repo_ctx().repo();
                let file_unode = file_unode_id.load(ctx, repo.repo_blobstore()).await?;
                let cs_id = file_unode.linknode().clone();
                Ok(Some(ChangesetContext::new(self.repo_ctx().clone(), cs_id)))
            }
            None => Ok(None),
        }
    }

    /// Returns the last commit that deleted this path.  If something exists
    /// at this path, or nothing ever existed at this path, returns `None`.
    pub async fn last_deleted(&self) -> Result<Option<ChangesetContext<R>>, MononokeError> {
        Ok(self
            .linknode()
            .await?
            .map(|cs_id| ChangesetContext::new(self.repo_ctx().clone(), cs_id)))
    }

    /// Blame metadata for this path.
    pub async fn blame(&self, follow_mutable_file_history: bool) -> Result<BlameV2, MononokeError> {
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo_ctx().repo();
        let csid = self.changeset.id();
        let (blame, _) =
            history_traversal::blame(ctx, repo, csid, &self.path, follow_mutable_file_history)
                .await?;
        Ok(blame)
    }

    /// Blame metadata for this path, and the content that was blamed.
    pub async fn blame_with_content(
        &self,
        follow_mutable_file_history: bool,
    ) -> Result<(BlameV2, Bytes), MononokeError> {
        let ctx = self.changeset.ctx();
        let repo = self.changeset.repo_ctx().repo();
        let csid = self.changeset.id();
        Ok(history_traversal::blame_with_content(
            ctx,
            repo,
            csid,
            &self.path,
            follow_mutable_file_history,
        )
        .await?)
    }

    /// Returns a list of `ChangesetContext` for the file at this path that represents
    /// a history of the path.
    pub async fn history(
        &self,
        ctx: &CoreContext,
        opts: ChangesetPathHistoryOptions,
    ) -> Result<BoxStream<'_, Result<ChangesetContext<R>, MononokeError>>, MononokeError> {
        let repo = self.repo_ctx().repo().clone();

        if let Some(descendants_of) = opts.descendants_of {
            if !repo
                .commit_graph()
                .is_ancestor(
                    self.changeset().ctx(),
                    descendants_of,
                    self.changeset().id(),
                )
                .watched(ctx.logger())
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
                            repo.repo_derived_data()
                                .derive::<ChangesetInfo>(ctx, cs_id)
                                .watched(ctx.logger())
                                .await
                        } else {
                            let bonsai = cs_id
                                .load(ctx, repo.repo_blobstore())
                                .watched(ctx.logger())
                                .await?;
                            Ok(ChangesetInfo::new(cs_id, bonsai))
                        }?;
                        let timestamp = info.author_date().as_chrono().timestamp();
                        Ok::<_, Error>((timestamp >= until_ts).then_some((cs_id, path)))
                    }))
                    .watched(ctx.logger())
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
                            .watched(ctx.logger())
                            .await?
                        {
                            anyhow::Ok(Some((cs_id, path)))
                        } else {
                            anyhow::Ok(None)
                        }
                    }))
                    .watched(ctx.logger())
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
                            .watched(ctx.logger())
                            .await?
                        {
                            Ok::<_, MononokeError>(None)
                        } else {
                            Ok::<_, MononokeError>(Some((cs_id, path)))
                        }
                    }))
                    .watched(ctx.logger())
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
                    Ok(self
                        ._visit(ctx, repo, descendant_cs_id, cs_ids)
                        .watched(ctx.logger())
                        .await?)
                }
            }

            async fn preprocess(
                &mut self,
                ctx: &CoreContext,
                repo: &impl history_traversal::Repo,
                descendant_id_cs_ids: Vec<(Option<CsAndPath>, Vec<CsAndPath>)>,
            ) -> Result<(), Error> {
                let items = stream::iter(descendant_id_cs_ids.into_iter())
                    .map(|(descendant_cs_id, cs_ids)| {
                        self._visit(ctx, repo, descendant_cs_id.clone(), cs_ids.clone())
                            .map_ok(move |res| ((descendant_cs_id, cs_ids), res))
                    })
                    .buffered(10)
                    .yield_periodically()
                    .try_collect::<Vec<_>>()
                    .await?;
                for (k, v) in items {
                    self.cache.insert(k, v);
                }
                Ok(())
            }
        }
        let cs_info_enabled = self.repo_ctx().derive_changeset_info_enabled();

        let history_across_deletions = if opts.follow_history_across_deletions {
            HistoryAcrossDeletions::Track
        } else {
            HistoryAcrossDeletions::DontTrack
        };

        let history = list_file_history(
            self.changeset.ctx(),
            self.repo_ctx().repo(),
            self.path.clone(),
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
            self.repo_ctx().mutable_renames().clone(),
            TraversalOrder::new_gen_num_order(
                self.changeset.ctx().clone(),
                repo.commit_graph_arc(),
            ),
        )
        .watched(ctx.logger())
        .await
        .map_err(|error| match error {
            FastlogError::InternalError(e) => MononokeError::from(anyhow!(e)),
            FastlogError::DerivationError(e) => MononokeError::from(e),
            FastlogError::LoadableError(e) => MononokeError::from(e),
            FastlogError::Error(e) => MononokeError::from(e),
        })?;

        Ok(history
            .map_err(MononokeError::from)
            .map_ok(move |changeset_id| {
                ChangesetContext::new(self.repo_ctx().clone(), changeset_id)
            })
            .boxed())
    }
}

impl<R: MononokeRepo> ChangesetPathContext<R> {
    pub(crate) async fn new(
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            entry_kind: LazyShared::new_empty(),
        })
    }

    pub(crate) async fn new_with_entry(
        changeset: ChangesetContext<R>,
        path: impl Into<MPath>,
        entry: Entry<(), ()>,
    ) -> Result<Self, MononokeError> {
        let path = path.into();
        changeset
            .repo_ctx()
            .authorization_context()
            .require_path_read(
                changeset.ctx(),
                changeset.repo_ctx().repo(),
                changeset.id(),
                &path,
            )
            .await?;
        Ok(Self {
            changeset,
            path,
            entry_kind: LazyShared::new_ready(Ok(Some(entry))),
        })
    }

    /// The `RepoContext` for this query.
    pub fn repo_ctx(&self) -> &RepoContext<R> {
        self.changeset.repo_ctx()
    }

    /// The `ChangesetContext` for this query.
    pub fn changeset(&self) -> &ChangesetContext<R> {
        &self.changeset
    }

    /// The path for this query.
    pub fn path(&self) -> &MPath {
        &self.path
    }

    async fn entry_kind(&self) -> Result<Option<Entry<(), ()>>, MononokeError> {
        self.entry_kind
            .get_or_init(|| {
                cloned!(self.changeset, self.path);
                async move {
                    let ctx = changeset.ctx().clone();
                    let blobstore = changeset.repo_ctx().repo().repo_blobstore().clone();
                    let repo_name = changeset.repo_ctx().name().to_string();

                    if let Some(mpath) = path.into_optional_non_root_path() {
                        if justknobs::eval(
                            "scm/mononoke:changeset_path_context_use_skeleton_manifest_v2",
                            None,
                            Some(&repo_name),
                        )? {
                            let root_skeleton_manifest_v2_id =
                                changeset.root_skeleton_manifest_v2_id().await?;
                            root_skeleton_manifest_v2_id
                                .into_inner_id()
                                .load(&ctx, &blobstore)
                                .await?
                                .find_entry(ctx, blobstore, MPath::from(mpath))
                                .await
                                .map(|maybe_entry| maybe_entry.map(|entry| entry.map_tree(|_| ())))
                                .map_err(MononokeError::from)
                        } else {
                            let root_skeleton_manifest_id =
                                changeset.root_skeleton_manifest_id().await?;
                            root_skeleton_manifest_id
                                .skeleton_manifest_id()
                                .find_entry(ctx, blobstore, MPath::from(mpath))
                                .await
                                .map(|maybe_entry| maybe_entry.map(|entry| entry.map_tree(|_| ())))
                                .map_err(MononokeError::from)
                        }
                    } else {
                        Ok(Some(Entry::Tree(())))
                    }
                }
            })
            .await
    }

    /// Returns `true` if the path exists (as a file or directory) in this commit.
    pub async fn exists(&self) -> Result<bool, MononokeError> {
        // The path exists if there is any kind of skeleton manifest entry.
        Ok(self.entry_kind().await?.is_some())
    }

    pub async fn is_file(&self) -> Result<bool, MononokeError> {
        let is_file = match self.entry_kind().await? {
            Some(Entry::Leaf(_)) => true,
            _ => false,
        };
        Ok(is_file)
    }

    pub async fn is_tree(&self) -> Result<bool, MononokeError> {
        let is_tree = match self.entry_kind().await? {
            Some(Entry::Tree(_)) => true,
            _ => false,
        };
        Ok(is_tree)
    }
}
