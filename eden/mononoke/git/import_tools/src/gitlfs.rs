/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use core::future::Future;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::format_err;
use bytes::Bytes;
use clientinfo::CLIENT_INFO_HEADER;
use clientinfo::ClientInfo;
use context::CoreContext;
use filestore::StoreRequest;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::git_lfs::LfsPointerData;
use git_types::git_lfs::parse_lfs_pointer;
use gix_hash::ObjectId;
use http::HeaderValue;
use http::Request;
use http::Uri;
use hyper::Body;
use hyper::Client;
use hyper::StatusCode;
use hyper::body;
use hyper::client::connect::HttpConnector;
use hyper_openssl::HttpsConnector;
use mononoke_macros::mononoke;
use mononoke_types::hash;
use openssl::ssl::SslConnector;
use openssl::ssl::SslFiletype;
use openssl::ssl::SslMethod;
use rand::Rng;
use rand::rng;
use tls::TLSArgs;
use tokio::sync::Semaphore;
use tokio::time::Duration;
use tokio::time::sleep;
use tracing::error;
use tracing::warn;

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
    ///   A non existing LFS file considered unrecoverable error and bail out
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GitLfsFetchResult {
    Fetched,
    NotFound,
}

impl GitLfsFetchResult {
    pub fn is_fetched(&self) -> bool {
        *self == GitLfsFetchResult::Fetched
    }

    pub fn is_not_found(&self) -> bool {
        *self == GitLfsFetchResult::NotFound
    }
}
#[derive(Clone, Debug, Default)]
pub struct GitImportLfs {
    inner: Option<Arc<GitImportLfsInner>>,
}

impl GitImportLfs {
    pub fn new_disabled() -> Self {
        GitImportLfs { inner: None }
    }
    pub fn new(
        lfs_server: String,
        allow_not_found: bool,
        max_attempts: u32,
        conn_limit: Option<usize>,
        tls_args: Option<TLSArgs>,
    ) -> Result<Self, Error> {
        let mut ssl_connector = SslConnector::builder(SslMethod::tls_client())?;
        if let Some(tls_args) = tls_args {
            ssl_connector.set_ca_file(tls_args.tls_ca)?;
            ssl_connector.set_certificate_file(tls_args.tls_certificate, SslFiletype::PEM)?;
            ssl_connector.set_private_key_file(tls_args.tls_private_key, SslFiletype::PEM)?;
        };
        let mut http_connector = HttpConnector::new();
        http_connector.enforce_http(false);
        let connector =
            HttpsConnector::with_connector(http_connector, ssl_connector).map_err(Error::from)?;

        let client: Client<_, body::Body> = Client::builder().build(connector);
        let inner = GitImportLfsInner {
            lfs_server,
            allow_not_found,
            max_attempts,
            time_ms_between_attempts: 10000,
            conn_limit_sem: conn_limit.map(|x| Arc::new(Semaphore::new(x))),
            client,
        };
        Ok(GitImportLfs {
            inner: Some(Arc::new(inner)),
        })
    }

    /// Checks whether given blob is valid Git LFS pointer and returns its metadata
    pub fn is_lfs_file(&self, gitblob: &[u8], gitid: ObjectId) -> Option<LfsPointerData> {
        if self.inner.is_some() {
            parse_lfs_pointer(gitblob, gitid)
        } else {
            None
        }
    }

    /// Download the LFS file. This works fine with Dewey but should be improved to work
    /// with other backends as well.
    async fn fetch_bytes_internal(
        &self,
        ctx: &CoreContext,
        metadata: &LfsPointerData,
    ) -> Result<
        (
            StoreRequest,
            impl Stream<Item = Result<Bytes, Error>> + Unpin + use<>,
            GitLfsFetchResult,
        ),
        Error,
    > {
        let inner = self.inner.as_ref().ok_or_else(|| {
            format_err!("GitImportLfs::fetch_bytes_internal called on disabled GitImportLfs")
        })?;

        let uri = [&inner.lfs_server, "/", &metadata.sha256.to_string()]
            .concat()
            .parse::<Uri>()?;
        let mut req = Request::get(uri.clone())
            .body(Body::empty())
            .context("creating LFS fetch request")?;
        let client_info = ctx
            .metadata()
            .client_info()
            .cloned()
            .unwrap_or_else(ClientInfo::default);
        req.headers_mut().insert(
            CLIENT_INFO_HEADER,
            HeaderValue::from_str(&client_info.to_json()?)?,
        );
        let resp = inner
            .client
            .request(req)
            .await
            .with_context(|| format!("fetch_bytes_internal {}", uri))?;

        if resp.status().is_success() {
            let bytes = resp.into_body().map_err(Error::from);
            let sr = StoreRequest::with_sha256(metadata.size, metadata.sha256);
            return Ok((sr, bytes.left_stream(), GitLfsFetchResult::Fetched));
        }
        if resp.status() == StatusCode::NOT_FOUND && inner.allow_not_found {
            warn!(
                "{} not found. Using gitlfs metadata as file content instead.",
                uri,
            );
            let bytes = Bytes::copy_from_slice(&metadata.gitblob);
            let size = metadata.gitblob.len().try_into()?;
            let git_sha1 = hash::RichGitSha1::from_bytes(
                Bytes::copy_from_slice(metadata.gitid.as_bytes()),
                "blob",
                size,
            )?;
            let sr = StoreRequest::with_git_sha1(size, git_sha1);
            return Ok((
                sr,
                stream::once(futures::future::ok(bytes)).right_stream(),
                GitLfsFetchResult::NotFound,
            ));
        }
        Err(format_err!("{} response {:?}", uri, resp))
    }

    async fn fetch_bytes(
        &self,
        ctx: &CoreContext,
        metadata: &LfsPointerData,
    ) -> Result<
        (
            StoreRequest,
            impl Stream<Item = Result<Bytes, Error>> + use<>,
            GitLfsFetchResult,
        ),
        Error,
    > {
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
                    // Sleep on average time_ms_between_attempts between attempts.
                    let sleep_time_ms = rng().random_range(0..inner.time_ms_between_attempts * 2);
                    error!(
                        "{}. Attempt {} of {} - Retrying in {} ms",
                        err, attempt, inner.max_attempts, sleep_time_ms,
                    );
                    sleep(Duration::from_millis(sleep_time_ms.into())).await;
                }
            }
        }
    }

    pub async fn with<F, T, Fut>(
        self,
        ctx: CoreContext,
        metadata: LfsPointerData,
        f: F,
    ) -> Result<T, Error>
    where
        F: FnOnce(
                CoreContext,
                LfsPointerData,
                StoreRequest,
                Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
                GitLfsFetchResult,
            ) -> Fut
            + Send
            + 'static,
        T: Send + Sync + 'static,
        Fut: Future<Output = Result<T, Error>> + Send,
    {
        mononoke::spawn_task(async move {
            let inner = self.inner.as_ref().ok_or_else(|| {
                format_err!("GitImportLfs::fetch_bytes_internal called on disabled GitImportLfs")
            })?;

            // If configured a connection limit, grab semaphore lock enforcing it.
            let _slock = if let Some(semaphore) = &inner.conn_limit_sem {
                Some(semaphore.clone().acquire_owned().await?)
            } else {
                None
            };

            let (req, bstream, fetch_result) = self.fetch_bytes(&ctx, &metadata).await?;
            f(ctx, metadata, req, Box::new(bstream), fetch_result).await
        })
        .await?
    }
}
