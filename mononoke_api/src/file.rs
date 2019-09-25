// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::future::Future;
use std::pin::Pin;

use cloned::cloned;
use failure::err_msg;
use filestore::{get_metadata, FetchKey};
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, Shared};

use crate::errors::MononokeError;
use crate::repo::RepoContext;

/// A file's ID is its content id.
pub use mononoke_types::ContentId as FileId;

/// The type of a file.
pub use mononoke_types::FileType;

/// Metadata about a file.
pub use mononoke_types::ContentMetadata as FileMetadata;

#[derive(Clone)]
pub struct FileContext {
    repo: RepoContext,
    metadata: Shared<Pin<Box<dyn Future<Output = Result<FileMetadata, MononokeError>> + Send>>>,
}

/// Context for accessing a file in a repository.
///
/// Files are content-addressed, so if the same file occurs in multiple
/// places in the repository, this context represents all of them. As such,
/// it's not possible to go back to the commit or path from a `FileContext`.
///
/// See `ChangesetPathContext` if you need to refer to a specific file in a
/// specific commit.
impl FileContext {
    /// Create a new FileContext.  The file must exist in the repository.
    ///
    /// To construct a `FileContext` for a file that might not exist, use
    /// `new_check_exists`.
    pub(crate) fn new(repo: RepoContext, fetch_key: FetchKey) -> Self {
        let metadata = {
            cloned!(repo);
            async move {
                get_metadata(
                    &repo.blob_repo().get_blobstore(),
                    repo.ctx().clone(),
                    &fetch_key,
                )
                .compat()
                .await
                .and_then(|metadata| {
                    metadata.ok_or_else(|| err_msg(format!("content not found: {:?}", fetch_key)))
                })
                .map_err(MononokeError::from)
            }
        };
        let metadata = metadata.boxed().shared();
        Self { repo, metadata }
    }

    /// Create a new  FileContext using an ID that might not exist. Returns
    /// `None` if the file doesn't exist.
    pub(crate) async fn new_check_exists(
        repo: RepoContext,
        fetch_key: FetchKey,
    ) -> Result<Option<Self>, MononokeError> {
        // Try to get the file metadata immediately to see if it exists.
        let file = get_metadata(
            &repo.blob_repo().get_blobstore(),
            repo.ctx().clone(),
            &fetch_key,
        )
        .compat()
        .await?
        .map(|metadata| {
            let metadata = async move { Ok(metadata) };
            let metadata = metadata.boxed().shared();
            Self { repo, metadata }
        });
        Ok(file)
    }

    /// Return the metadata for a file.
    pub async fn metadata(&self) -> Result<FileMetadata, MononokeError> {
        self.metadata.clone().await
    }
}
