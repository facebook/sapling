/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use blobstore::Loadable;
use blobstore::LoadableError;
use cloned::cloned;
use futures_lazy_shared::LazyShared;
use mononoke_types::fsnode::Fsnode;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

// Trees are identified by their FsnodeId.
pub use mononoke_types::FsnodeId as TreeId;

// An entry within a tree list (either a file or subdirectory).
pub use mononoke_types::fsnode::FsnodeEntry as TreeEntry;

// Summary information about the files in a tree.
pub use mononoke_types::fsnode::FsnodeSummary as TreeSummary;

#[derive(Clone)]
pub struct TreeContext {
    repo: RepoContext,
    id: TreeId,
    fsnode: LazyShared<Result<Fsnode, MononokeError>>,
}

impl fmt::Debug for TreeContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "TreeContext(repo={:?} id={:?})",
            self.repo().name(),
            self.id()
        )
    }
}

impl TreeContext {
    /// Create a new TreeContext. The tree must exist in the repo and have
    /// had its derived data generated.
    ///
    /// To construct a `TreeContext` for a tree that might not exist, use
    /// `new_check_exists`.
    pub(crate) fn new(repo: RepoContext, id: TreeId) -> Self {
        Self {
            repo,
            id,
            fsnode: LazyShared::new_empty(),
        }
    }

    /// Create a new TreeContext using an ID that might not exist. Returns
    /// `None` if the tree doesn't exist.
    pub(crate) async fn new_check_exists(
        repo: RepoContext,
        id: TreeId,
    ) -> Result<Option<Self>, MononokeError> {
        // Try to load the fsnode immediately to see if it exists. Unlike
        // `new`, if the fsnode is missing, we simply return `Ok(None)`.
        match id.load(repo.ctx(), repo.blob_repo().blobstore()).await {
            Ok(fsnode) => Ok(Some(Self {
                repo,
                id,
                fsnode: LazyShared::new_ready(Ok(fsnode)),
            })),
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(e) => Err(MononokeError::from(Error::from(e))),
        }
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    async fn fsnode(&self) -> Result<Fsnode, MononokeError> {
        self.fsnode
            .get_or_init(|| {
                cloned!(self.repo, self.id);
                async move {
                    id.load(repo.ctx(), repo.blob_repo().blobstore())
                        .await
                        .map_err(Error::from)
                        .map_err(MononokeError::from)
                }
            })
            .await
    }

    pub fn id(&self) -> &TreeId {
        &self.id
    }

    pub async fn summary(&self) -> Result<TreeSummary, MononokeError> {
        let summary = self.fsnode().await?.summary().clone();
        Ok(summary)
    }

    pub async fn list(&self) -> Result<impl Iterator<Item = (String, TreeEntry)>, MononokeError> {
        let fsnode = self.fsnode().await?;
        let entries = fsnode
            .into_subentries()
            .into_iter()
            .map(|(elem, entry)| (String::from_utf8_lossy(elem.as_ref()).to_string(), entry));
        Ok(entries)
    }
}
