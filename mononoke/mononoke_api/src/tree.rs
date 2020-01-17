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

use anyhow::Error;
use blobstore::{Loadable, LoadableError};
use cloned::cloned;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, Shared};
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
    fsnode: Shared<Pin<Box<dyn Future<Output = Result<Fsnode, MononokeError>> + Send>>>,
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
        let fsnode = {
            cloned!(repo);
            async move {
                id.load(repo.ctx().clone(), repo.blob_repo().blobstore())
                    .compat()
                    .await
                    .map_err(Error::from)
                    .map_err(MononokeError::from)
            }
        };
        let fsnode = fsnode.boxed().shared();
        Self { repo, id, fsnode }
    }

    /// Create a new TreeContext using an ID that might not exist. Returns
    /// `None` if the tree doesn't exist.
    pub(crate) async fn new_check_exists(
        repo: RepoContext,
        id: TreeId,
    ) -> Result<Option<Self>, MononokeError> {
        // Try to load the fsnode immediately to see if it exists. Unlike
        // `new`, if the fsnode is missing, we simply return `Ok(None)`.
        match id
            .load(repo.ctx().clone(), repo.blob_repo().blobstore())
            .compat()
            .await
        {
            Ok(fsnode) => {
                let fsnode = async move { Ok(fsnode) };
                let fsnode = fsnode.boxed().shared();
                Ok(Some(Self { repo, id, fsnode }))
            }
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(e) => Err(MononokeError::from(Error::from(e))),
        }
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    async fn fsnode(&self) -> Result<Fsnode, MononokeError> {
        self.fsnode.clone().await
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
