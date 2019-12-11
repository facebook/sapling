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

use anyhow::format_err;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use filestore::{fetch, fetch_range, get_metadata, FetchKey};
use futures::stream::{self, Stream};
use futures_ext::StreamExt;
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
    fetch_key: FetchKey,
    metadata: Shared<Pin<Box<dyn Future<Output = Result<FileMetadata, MononokeError>> + Send>>>,
}

impl fmt::Debug for FileContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "FileContext(repo={:?} fetch_key={:?})",
            self.repo().name(),
            self.fetch_key
        )
    }
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
            cloned!(repo, fetch_key);
            async move {
                get_metadata(repo.blob_repo().blobstore(), repo.ctx().clone(), &fetch_key)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
                    .and_then(|metadata| {
                        metadata.ok_or_else(|| content_not_found_error(&fetch_key))
                    })
            }
        };
        let metadata = metadata.boxed().shared();
        Self {
            repo,
            fetch_key,
            metadata,
        }
    }

    /// Create a new  FileContext using an ID that might not exist. Returns
    /// `None` if the file doesn't exist.
    pub(crate) async fn new_check_exists(
        repo: RepoContext,
        fetch_key: FetchKey,
    ) -> Result<Option<Self>, MononokeError> {
        // Try to get the file metadata immediately to see if it exists.
        let file = get_metadata(repo.blob_repo().blobstore(), repo.ctx().clone(), &fetch_key)
            .compat()
            .await?
            .map(|metadata| {
                let metadata = async move { Ok(metadata) };
                let metadata = metadata.boxed().shared();
                Self {
                    repo,
                    fetch_key,
                    metadata,
                }
            });
        Ok(file)
    }

    /// The context for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.repo.ctx()
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// Return the metadata for a file.
    pub async fn metadata(&self) -> Result<FileMetadata, MononokeError> {
        self.metadata.clone().await
    }

    /// Return a stream of the content for the file.
    pub async fn content(&self) -> impl Stream<Item = Bytes, Error = MononokeError> {
        let stream = fetch(
            self.repo().blob_repo().blobstore(),
            self.ctx().clone(),
            &self.fetch_key,
        )
        .compat()
        .await;
        match stream {
            Ok(Some(stream)) => stream.map_err(MononokeError::from).left_stream(),
            Ok(None) => stream::once(Err(content_not_found_error(&self.fetch_key))).right_stream(),
            Err(e) => stream::once(Err(MononokeError::from(e))).right_stream(),
        }
    }

    /// Return a stream of the content for a range within the file.
    ///
    /// If the range goes past the end of the file, then content up to
    /// the end of the file is returned.  If the range starts past the
    /// end of the file, then an empty stream is returned.
    pub async fn content_range(
        &self,
        start: u64,
        size: u64,
    ) -> impl Stream<Item = Bytes, Error = MononokeError> {
        let stream = fetch_range(
            self.repo().blob_repo().blobstore(),
            self.ctx().clone(),
            &self.fetch_key,
            start,
            size,
        )
        .compat()
        .await;
        match stream {
            Ok(Some(stream)) => stream.map_err(MononokeError::from).left_stream(),
            Ok(None) => stream::once(Err(content_not_found_error(&self.fetch_key))).right_stream(),
            Err(e) => stream::once(Err(MononokeError::from(e))).right_stream(),
        }
    }
}

/// File contexts should only exist for files that are known to be in the
/// blobstore. If attempting to access the content results in an error, this
/// error is returned. This is an internal error, as it means either the data
/// has been lost from the blobstore, or the file context was erroneously
/// constructed.
fn content_not_found_error(fetch_key: &FetchKey) -> MononokeError {
    MononokeError::from(format_err!("content not found: {:?}", fetch_key))
}
