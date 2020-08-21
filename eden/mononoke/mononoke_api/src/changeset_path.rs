/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::identity;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{format_err, Error};
use async_trait::async_trait;
use blame::{fetch_blame, BlameError};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fastlog::{list_file_history, FastlogError, HistoryAcrossDeletions, Visitor};
use filestore::FetchKey;
use futures::compat::Future01CompatExt;
use futures::future::{try_join_all, FutureExt, Shared, TryFutureExt};
use futures::stream::{Stream, TryStreamExt};
use futures::try_join;
use manifest::{Entry, ManifestOps};
use mononoke_types::{
    Blame, ChangesetId, ContentId, FileType, FileUnodeId, FsnodeId, Generation, ManifestUnodeId,
};
use reachabilityindex::ReachabilityIndex;
use skiplist::SkiplistIndex;
use std::collections::HashMap;
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

type FsnodeResult = Result<Option<Entry<FsnodeId, (ContentId, FileType)>>, MononokeError>;
type UnodeResult = Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError>;

/// A path within a changeset.
///
/// A ChangesetPathContext may represent a file, a directory, a path where a
/// file or directory has been deleted, or a path where nothing ever existed.
#[derive(Clone)]
pub struct ChangesetPathContext {
    changeset: ChangesetContext,
    path: MononokePath,
    fsnode_id: Shared<Pin<Box<dyn Future<Output = FsnodeResult> + Send>>>,
    unode_id: Shared<Pin<Box<dyn Future<Output = UnodeResult> + Send>>>,
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

impl ChangesetPathContext {
    fn new_impl(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        fsnode_entry: Option<Entry<FsnodeId, (ContentId, FileType)>>,
    ) -> Self {
        let path = path.into();
        let fsnode_id = if let Some(fsnode_entry) = fsnode_entry {
            async move { Ok(Some(fsnode_entry)) }.boxed()
        } else {
            cloned!(changeset, path);
            async move {
                let ctx = changeset.ctx().clone();
                let blobstore = changeset.repo().blob_repo().get_blobstore();
                let root_fsnode_id = changeset.root_fsnode_id().await?;
                if let Some(mpath) = path.into() {
                    root_fsnode_id
                        .fsnode_id()
                        .find_entry(ctx, blobstore, Some(mpath))
                        .compat()
                        .await
                        .map_err(MononokeError::from)
                } else {
                    Ok(Some(Entry::Tree(root_fsnode_id.fsnode_id().clone())))
                }
            }
            .boxed()
        };
        let fsnode_id = fsnode_id.shared();
        let unode_id = {
            cloned!(changeset, path);
            async move {
                let blobstore = changeset.repo().blob_repo().get_blobstore();
                let ctx = changeset.ctx().clone();
                let root_unode_manifest_id = changeset.root_unode_manifest_id().await?;
                if let Some(mpath) = path.into() {
                    root_unode_manifest_id
                        .manifest_unode_id()
                        .find_entry(ctx.clone(), blobstore.clone(), Some(mpath))
                        .compat()
                        .await
                        .map_err(MononokeError::from)
                } else {
                    Ok(Some(Entry::Tree(
                        root_unode_manifest_id.manifest_unode_id().clone(),
                    )))
                }
            }
        };
        let unode_id = unode_id.boxed().shared();
        Self {
            changeset,
            path,
            fsnode_id,
            unode_id,
        }
    }

    pub(crate) fn new(changeset: ChangesetContext, path: impl Into<MononokePath>) -> Self {
        Self::new_impl(changeset, path, None)
    }

    pub(crate) fn new_with_fsnode_entry(
        changeset: ChangesetContext,
        path: impl Into<MononokePath>,
        fsnode_entry: Entry<FsnodeId, (ContentId, FileType)>,
    ) -> Self {
        Self::new_impl(changeset, path, Some(fsnode_entry))
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

    async fn fsnode_id(
        &self,
    ) -> Result<Option<Entry<FsnodeId, (ContentId, FileType)>>, MononokeError> {
        self.fsnode_id.clone().await
    }

    #[allow(dead_code)]
    async fn unode_id(&self) -> Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError> {
        self.unode_id.clone().await
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

    pub async fn is_dir(&self) -> Result<bool, MononokeError> {
        let is_dir = match self.fsnode_id().await? {
            Some(Entry::Tree(_)) => true,
            _ => false,
        };
        Ok(is_dir)
    }

    pub async fn file_type(&self) -> Result<Option<FileType>, MononokeError> {
        let file_type = match self.fsnode_id().await? {
            Some(Entry::Leaf((_content_id, file_type))) => Some(file_type),
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
            Some(Entry::Leaf((content_id, _file_type))) => Some(FileContext::new(
                self.repo().clone(),
                FetchKey::Canonical(content_id),
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
            Some(Entry::Leaf((content_id, file_type))) => PathEntry::File(
                FileContext::new(self.repo().clone(), FetchKey::Canonical(content_id)),
                file_type,
            ),
            _ => PathEntry::NotPresent,
        };
        Ok(entry)
    }

    pub async fn blame(&self) -> Result<(Bytes, Blame), MononokeError> {
        let ctx = self.changeset.ctx().clone();
        let repo = self.changeset.repo().blob_repo().clone();
        let csid = self.changeset.id();
        let mpath = self.path.as_mpath().ok_or_else(|| {
            MononokeError::InvalidRequest(format!("Blame is not available for directory: `/`"))
        })?;

        fetch_blame(ctx, repo, csid, mpath.clone())
            .compat()
            .map_err(|error| match error {
                BlameError::NoSuchPath(_)
                | BlameError::IsDirectory(_)
                | BlameError::Rejected(_) => MononokeError::InvalidRequest(error.to_string()),
                BlameError::DeriveError(e) => MononokeError::from(e),
                _ => MononokeError::from(Error::from(error)),
            })
            .await
    }

    /// Returns a list of `ChangesetContext` for the file at this path that represents
    /// a history of the path.
    pub async fn history(
        &self,
        until_timestamp: Option<i64>,
        descendants_of: Option<ChangesetId>,
        follow_history_across_deletions: bool,
    ) -> Result<impl Stream<Item = Result<ChangesetContext, MononokeError>> + '_, MononokeError>
    {
        let ctx = self.changeset.ctx().clone();
        let repo = self.repo().blob_repo().clone();
        let mpath = self.path.as_mpath();

        let descendants_of = match descendants_of {
            Some(descendants_of) => Some((
                descendants_of,
                repo.get_changeset_fetcher()
                    .get_generation_number(ctx.clone(), descendants_of)
                    .compat()
                    .await?,
            )),
            None => None,
        };

        struct FilterVisitor {
            cs_info_enabled: bool,
            until_timestamp: Option<i64>,
            descendants_of: Option<(ChangesetId, Generation)>,
            cache: HashMap<(Option<ChangesetId>, Vec<ChangesetId>), Vec<ChangesetId>>,
            skiplist_index: Arc<SkiplistIndex>,
        };
        impl FilterVisitor {
            async fn _visit(
                &self,
                ctx: &CoreContext,
                repo: &BlobRepo,
                descendant_cs_id: Option<ChangesetId>,
                mut cs_ids: Vec<ChangesetId>,
            ) -> Result<Vec<ChangesetId>, Error> {
                let cs_info_enabled = self.cs_info_enabled;
                let skiplist_index = self.skiplist_index.clone();
                if let Some(until_ts) = self.until_timestamp {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|cs_id| async move {
                        let info = if cs_info_enabled {
                            ChangesetInfo::derive(ctx.clone(), repo.clone(), cs_id)
                                .compat()
                                .await
                        } else {
                            let bonsai = cs_id.load(ctx.clone(), repo.blobstore()).await?;
                            Ok(ChangesetInfo::new(cs_id, bonsai))
                        }?;
                        let timestamp = info.author_date().as_chrono().timestamp();
                        Ok::<_, Error>((timestamp >= until_ts).then_some(cs_id))
                    }))
                    .await?
                    .into_iter()
                    .filter_map(identity)
                    .collect();
                }
                if let Some((descendants_of, descendants_of_gen)) = self.descendants_of {
                    cs_ids = try_join_all(cs_ids.into_iter().map(|cs_id| {
                        cloned!(skiplist_index);
                        async move {
                            let changeset_fetcher = repo.get_changeset_fetcher();
                            let cs_gen = changeset_fetcher
                                .get_generation_number(ctx.clone(), cs_id)
                                .compat()
                                .await?;
                            if cs_gen < descendants_of_gen {
                                return Ok(None);
                            }
                            let ancestry_check_needed =
                                if let Some(descendant_cs_id) = descendant_cs_id {
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
                            Ok::<_, Error>(is_descendant.then_some(cs_id))
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
                descendant_cs_id: Option<ChangesetId>,
                cs_ids: Vec<ChangesetId>,
            ) -> Result<Vec<ChangesetId>, Error> {
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
                descendant_id_cs_ids: Vec<(Option<ChangesetId>, Vec<ChangesetId>)>,
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
        };
        let cs_info_enabled = self.repo().derive_changeset_info_enabled();

        let history_across_deletions = if follow_history_across_deletions {
            HistoryAcrossDeletions::Track
        } else {
            HistoryAcrossDeletions::DontTrack
        };
        let history = list_file_history(
            ctx,
            repo,
            mpath.cloned(),
            self.changeset.id(),
            FilterVisitor {
                cs_info_enabled,
                until_timestamp,
                descendants_of,
                cache: HashMap::new(),
                skiplist_index: self.repo().skiplist_index().clone(),
            },
            history_across_deletions,
        )
        .await
        .map_err(|error| match error {
            FastlogError::InternalError(e) => MononokeError::from(format_err!(e)),
            FastlogError::DeriveError(e) => MononokeError::from(e),
            FastlogError::LoadableError(e) => MononokeError::from(e),
            FastlogError::Error(e) => MononokeError::from(e),
        })?;

        Ok(history
            .map_err(MononokeError::from)
            .map_ok(move |changeset_id| ChangesetContext::new(self.repo().clone(), changeset_id)))
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
    old_path: &Option<ChangesetPathContext>,
    new_path: &Option<ChangesetPathContext>,
    copy_info: CopyInfo,
    context_lines: usize,
    mode: UnifiedDiffMode,
) -> Result<UnifiedDiff, MononokeError> {
    // Helper for getting file information.
    async fn get_file_data(
        path: &Option<ChangesetPathContext>,
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

    let (old_diff_file, new_diff_file) = try_join!(
        get_file_data(&old_path, mode),
        get_file_data(&new_path, mode)
    )?;
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
