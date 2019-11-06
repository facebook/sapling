/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use cloned::cloned;
use filestore::FetchKey;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, Shared};
use manifest::{Entry, ManifestOps};
use mononoke_types::{
    ChangesetId, ContentId, FileType, FileUnodeId, FsnodeId, MPath, ManifestUnodeId,
};

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileContext;
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

type FsnodeResult = Result<Option<Entry<FsnodeId, (ContentId, FileType)>>, MononokeError>;
type UnodeResult = Result<Option<Entry<ManifestUnodeId, FileUnodeId>>, MononokeError>;

/// A path within a changeset.
///
/// A ChangesetPathContext may represent a file, a directory, a path where a
/// file or directory has been deleted, or a path where nothing ever existed.
#[derive(Clone)]
pub struct ChangesetPathContext {
    changeset: ChangesetContext,
    mpath: Option<MPath>,
    fsnode_id: Shared<Pin<Box<dyn Future<Output = FsnodeResult> + Send>>>,
    unode_id: Shared<Pin<Box<dyn Future<Output = UnodeResult> + Send>>>,
}

impl ChangesetPathContext {
    pub(crate) fn new(changeset: ChangesetContext, mpath: Option<MPath>) -> Self {
        let fsnode_id = {
            cloned!(changeset, mpath);
            async move {
                let ctx = changeset.ctx().clone();
                let blobstore = changeset.repo().blob_repo().get_blobstore();
                let root_fsnode_id = changeset.root_fsnode_id().await?;
                if let Some(mpath) = mpath {
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
        };
        let fsnode_id = fsnode_id.boxed().shared();
        let unode_id = {
            cloned!(changeset, mpath);
            async move {
                let blobstore = changeset.repo().blob_repo().get_blobstore();
                let ctx = changeset.ctx().clone();
                let root_unode_manifest_id = changeset.root_unode_manifest_id().await?;
                if let Some(mpath) = mpath {
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
            mpath,
            fsnode_id,
            unode_id,
        }
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.changeset.repo()
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
}

impl fmt::Display for ChangesetPathContext {
    /// Returns a slash-separated `String` path suitable for returning to API user.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref mpath) = self.mpath {
            return write!(f, "{}", mpath);
        }
        write!(f, "")
    }
}
