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
// An entry within a tree list (either a file or subdirectory).
pub use mononoke_types::fsnode::FsnodeEntry as TreeEntry;
// Summary information about the files in a tree.
pub use mononoke_types::fsnode::FsnodeSummary as TreeSummary;
// Trees are identified by their FsnodeId.
pub use mononoke_types::FsnodeId as TreeId;
use repo_blobstore::RepoBlobstoreRef;

use crate::errors::MononokeError;
use crate::repo::MononokeRepo;
use crate::repo::RepoContext;

#[derive(Clone)]
pub struct TreeContext<R> {
    repo_ctx: RepoContext<R>,
    id: TreeId,
    fsnode: LazyShared<Result<Fsnode, MononokeError>>,
}

impl<R: MononokeRepo> fmt::Debug for TreeContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "TreeContext(repo_ctx={:?} id={:?})",
            self.repo_ctx().name(),
            self.id()
        )
    }
}

impl<R: MononokeRepo> TreeContext<R> {
    /// Create a new TreeContext. The tree must exist in the repo and have
    /// had its derived data generated, and the user must be known to have
    /// permission to access the file.
    ///
    /// To construct a `TreeContext` for a tree that might not exist, use
    /// `new_check_exists`.
    pub(crate) fn new_authorized(repo_ctx: RepoContext<R>, id: TreeId) -> Self {
        Self {
            repo_ctx,
            id,
            fsnode: LazyShared::new_empty(),
        }
    }

    /// Create a new TreeContext using an ID that might not exist. Returns
    /// `None` if the tree doesn't exist.
    pub(crate) async fn new_check_exists(
        repo_ctx: RepoContext<R>,
        id: TreeId,
    ) -> Result<Option<Self>, MononokeError> {
        // Access to an arbitrary tree requires full access to the repo,
        // as we do not know which path it corresponds to.
        repo_ctx
            .authorization_context()
            .require_full_repo_read(repo_ctx.ctx(), repo_ctx.repo())
            .await?;
        // Try to load the fsnode immediately to see if it exists. Unlike
        // `new`, if the fsnode is missing, we simply return `Ok(None)`.
        match id
            .load(repo_ctx.ctx(), repo_ctx.repo().repo_blobstore())
            .await
        {
            Ok(fsnode) => Ok(Some(Self {
                repo_ctx,
                id,
                fsnode: LazyShared::new_ready(Ok(fsnode)),
            })),
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(e) => Err(MononokeError::from(Error::from(e))),
        }
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo_ctx(&self) -> &RepoContext<R> {
        &self.repo_ctx
    }

    async fn fsnode(&self) -> Result<Fsnode, MononokeError> {
        self.fsnode
            .get_or_init(|| {
                cloned!(self.repo_ctx, self.id);
                async move {
                    id.load(repo_ctx.ctx(), repo_ctx.repo().repo_blobstore())
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
