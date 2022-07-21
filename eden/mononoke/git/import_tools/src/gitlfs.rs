/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use bytes::Bytes;
use context::CoreContext;
use core::future::Future;
use filestore::StoreRequest;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_hash::ObjectId;
use http::Uri;
use hyper::body;
use hyper::client::connect::HttpConnector;
use hyper::Client;
use hyper::StatusCode;
use hyper_openssl::HttpsConnector;
use mononoke_types::hash;
use rand::thread_rng;
use rand::Rng;
use slog::error;
use slog::warn;
use std::str;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tokio::time::Duration;
/// We will not try to parse any file bigger then this.
/// Any valid gitlfs metadata file should be smaller then this.
const MAX_METADATA_LENGTH: usize = 511;

/// Module to be passed into gitimport that defines how LFS files are imported.
/// The default will disable any LFS support (and the metadata of files pointing to LFS files
/// will be imported, this means that the mononoke repo will mirror the git-repo).
/// Autodetect and each file under MAX_METADATA_LENGTH will be scanned, and if it matched git-lfs
/// metadata file, then the configured lfs_server will be used to try and fetch the data.
#[derive(Debug)]
pub struct GitImportLfsInner {
    /// Server information.
    lfs_server: String,
    /// How to deal with the case when the file does not exist on the LFS server.
    /// allow_not_found=false
    ///   A non existing LFS file considerd unrecoverable error and bail out
    /// allow_not_found=true
    ///   put the content of the LFS-metafile in its place, and print a warning.
    allow_not_found: bool,
    /// Retries.
    max_attempts: u32,
    time_ms_between_attempts: u32,
    /// Limit the amount of simultaneous connections.
    conn_limit_sem: Option<Arc<Semaphore>>,
    /// Hyperium client we use to connect with
    client: Client<HttpsConnector<HttpConnector>>,
}

#[derive(Clone, Debug, Default)]
pub struct GitImportLfs {
    inner: Option<Arc<GitImportLfsInner>>,
}

#[derive(Debug)]
pub struct LfsMetaData {
    pub version: String,
    pub sha256: hash::Sha256,
    pub size: u64,
    /// gitblob and gitid, where this metadata comes from. This is useful if we
    /// end up storing the metadata instead of the content (if the content cannot
    /// be found on the LFS server for example).
    pub gitblob: Vec<u8>,
    pub gitid: ObjectId,
}

/// Layout of the metafiles:
/// | version https://git-lfs.github.com/spec/v1
/// | oid sha256:73e2200459562bb068f08e33210ed106014b877f878932b2147991e17a7c089b
/// | size 8423391
fn parse_lfs_metafile(gitblob: &[u8], gitid: ObjectId) -> Option<LfsMetaData> {
    if gitblob.len() > MAX_METADATA_LENGTH {
        return None;
    }

    let mut lines = str::from_utf8(gitblob).ok()?.lines();
    let version = lines.next()?.strip_prefix("version ")?;
    if version != "https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let sha256 = lines
        .next()?
        .strip_prefix("oid sha256:")?
        .parse::<hash::Sha256>()
        .ok()?;
    let size = lines.next()?.strip_prefix("size ")?.parse::<u64>().ok()?;
    // As a precaution. If we have an additional line after this, then we assume its not a valid file.
    if lines.next().is_some() {
        return None;
    }
    Some(LfsMetaData {
        version: version.to_string(),
        sha256,
        size,
        gitblob: gitblob.to_vec(),
        gitid,
    })
}

impl GitImportLfs {
    pub fn new_disabled() -> Self {
        GitImportLfs { inner: None }
    }
    pub fn new(
        lfs_server: String,
        allow_not_found: bool,
        conn_limit: Option<usize>,
    ) -> Result<Self, Error> {
        let connector = HttpsConnector::new().map_err(Error::from)?;
        let client: Client<_, body::Body> = Client::builder().build(connector);
        let inner = GitImportLfsInner {
            lfs_server,
            allow_not_found,
            max_attempts: 30,
            time_ms_between_attempts: 10000,
            conn_limit_sem: conn_limit.map(|x| Arc::new(Semaphore::new(x))),
            client,
        };
        Ok(GitImportLfs {
            inner: Some(Arc::new(inner)),
        })
    }

    pub fn is_lfs_file(&self, gitblob: &[u8], gitid: ObjectId) -> Option<LfsMetaData> {
        if self.inner.is_some() {
            parse_lfs_metafile(gitblob, gitid)
        } else {
            None
        }
    }

    /// Download the LFS file. This works fine with Dewey but should be improved to work
    /// with other backends as well.
    async fn fetch_bytes_internal(
        &self,
        ctx: &CoreContext,
        metadata: &LfsMetaData,
    ) -> Result<
        (
            StoreRequest,
            impl Stream<Item = Result<Bytes, Error>> + Unpin,
        ),
        Error,
    > {
        let inner = self.inner.as_ref().ok_or_else(|| {
            format_err!("GitImportLfs::fetch_bytes_internal called on disabled GitImportLfs")
        })?;

        let uri = [&inner.lfs_server, "/", &metadata.sha256.to_string()]
            .concat()
            .parse::<Uri>()?;
        let resp = inner.client.get(uri.clone()).await?;

        if resp.status().is_success() {
            let bytes = resp.into_body().map_err(Error::from);
            let sr = StoreRequest::with_sha256(metadata.size, metadata.sha256);
            return Ok((sr, bytes.left_stream()));
        }
        if resp.status() == StatusCode::NOT_FOUND && inner.allow_not_found {
            warn!(
                ctx.logger(),
                "{} not found. Using gitlfs metadata as file content instead.", uri,
            );
            let bytes = Bytes::copy_from_slice(&metadata.gitblob);
            let size = metadata.gitblob.len().try_into()?;
            let git_sha1 = hash::RichGitSha1::from_bytes(
                Bytes::copy_from_slice(metadata.gitid.as_bytes()),
                "blob",
                size,
            )?;
            let sr = StoreRequest::with_git_sha1(size, git_sha1);
            return Ok((sr, stream::once(futures::future::ok(bytes)).right_stream()));
        }
        Err(format_err!("{} response {:?}", uri, resp))
    }

    async fn fetch_bytes(
        &self,
        ctx: &CoreContext,
        metadata: &LfsMetaData,
    ) -> Result<(StoreRequest, impl Stream<Item = Result<Bytes, Error>>), Error> {
        let inner = self.inner.as_ref().ok_or_else(|| {
            format_err!("GitImportLfs::fetch_bytes called on disabled GitImportLfs")
        })?;

        let mut attempt: u32 = 0;
        loop {
            let r = self.fetch_bytes_internal(ctx, metadata).await;
            match r {
                Ok(res) => {
                    return Ok(res);
                }
                Err(err) => {
                    if attempt >= inner.max_attempts {
                        return Err(err);
                    }

                    attempt += 1;
                    error!(
                        ctx.logger(),
                        "{}. Attempt {} of {} - Retrying in {} ms",
                        err,
                        attempt,
                        inner.max_attempts,
                        inner.time_ms_between_attempts,
                    );
                    // Sleep on avarage time_ms_between_attempts between attempts.
                    let sleep_time_ms =
                        thread_rng().gen_range(0..inner.time_ms_between_attempts * 2);
                    sleep(Duration::from_millis(sleep_time_ms.into())).await;
                }
            }
        }
    }

    pub async fn with<F, T, Fut>(
        self,
        ctx: CoreContext,
        metadata: LfsMetaData,
        f: F,
    ) -> Result<T, Error>
    where
        F: FnOnce(
                CoreContext,
                LfsMetaData,
                StoreRequest,
                Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
            ) -> Fut
            + Send
            + 'static,
        T: Send + Sync + 'static,
        Fut: Future<Output = Result<T, Error>> + Send,
    {
        tokio::spawn(async move {
            let inner = self.inner.as_ref().ok_or_else(|| {
                format_err!("GitImportLfs::fetch_bytes_internal called on disabled GitImportLfs")
            })?;

            // If configured a connection limit, grab semaphore lock enforcing it.
            let _slock = if let Some(semaphore) = &inner.conn_limit_sem {
                Some(semaphore.clone().acquire_owned().await?)
            } else {
                None
            };

            let (req, bstream) = self.fetch_bytes(&ctx, &metadata).await?;
            f(ctx, metadata, req, Box::new(bstream)).await
        })
        .await?
    }
}
