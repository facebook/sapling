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
use blobstore::KeyedBlobstore;
use bytes::Bytes;
use clientinfo::CLIENT_INFO_HEADER;
use clientinfo::ClientInfo;
use context::CoreContext;
use filestore::Alias;
use filestore::FetchKey;
use filestore::StoreRequest;
use futures::Stream;
use futures::TryStreamExt;
use futures::stream;
use git_types::git_lfs::LfsPointerData;
use git_types::git_lfs::parse_lfs_pointer;
use gix_hash::ObjectId;
use http::HeaderValue;
use http::Request;
use http::StatusCode;
use http::Uri;
use http_body_util::BodyExt as _;
use http_body_util::Full;
use hyper_openssl::client::legacy::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use mononoke_macros::mononoke;
use mononoke_types::hash;
use openssl::ssl::SslConnector;
use openssl::ssl::SslFiletype;
use openssl::ssl::SslMethod;
use repourl::encode_repo_name;
use tls::TLSArgs;
use tokio::sync::Semaphore;
use tokio::time::Duration;
use tokio::time::sleep;
use tracing::error;
use tracing::warn;

/// URL pattern used by the upstream LFS server to serve a single object keyed by SHA256.
/// `LegacyDewey` matches Dewey's bare-suffix scheme; `MononokeGitLfs` matches the
/// Mononoke LFS server's `/{repo}/download_sha256/{oid}` route.
#[derive(Clone, Debug, Default)]
pub enum LfsServerUrlFormat {
    /// `GET {server}/{sha256}`
    #[default]
    LegacyDewey,
    /// `GET {server}/{repo_name}/download_sha256/{sha256}`
    MononokeGitLfs { repo_name: String },
}

impl LfsServerUrlFormat {
    fn build_object_url(&self, lfs_server: &str, sha256: &hash::Sha256) -> Result<Uri, Error> {
        let base = lfs_server.trim_end_matches('/');
        let url = match self {
            Self::LegacyDewey => format!("{base}/{sha256}"),
            Self::MononokeGitLfs { repo_name } => {
                format!(
                    "{base}/{}/download_sha256/{sha256}",
                    encode_repo_name(repo_name),
                )
            }
        };
        url.parse::<Uri>().map_err(Error::from)
    }
}

/// Module to be passed into gitimport that defines how LFS files are imported.
/// The default will disable any LFS support (and the metadata of files pointing to LFS files
/// will be imported, this means that the mononoke repo will mirror the git-repo).
/// Autodetect and each file under MAX_METADATA_LENGTH will be scanned, and if it matched git-lfs
/// metadata file, then either the configured upstream lfs_server or the local Mononoke filestore
/// will be used to fetch the data.
#[derive(Debug)]
pub enum GitImportLfsInner {
    /// Fetch LFS object bytes over HTTP from an upstream LFS server.
    Upstream(UpstreamLfs),
    /// Fetch LFS object bytes directly from the local Mononoke filestore by SHA256 alias.
    Internal(InternalLfs),
}

#[derive(Debug)]
pub struct UpstreamLfs {
    /// Server information.
    lfs_server: String,
    /// URL pattern used to construct per-object fetch URLs.
    url_format: LfsServerUrlFormat,
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
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
}

#[derive(Debug)]
pub struct InternalLfs {
    /// The local Mononoke blobstore used to look up LFS objects by SHA256 alias.
    blobstore: Arc<dyn KeyedBlobstore>,
    /// Same semantics as `UpstreamLfs::allow_not_found`, but checks the local filestore
    /// instead of an HTTP response. When true, missing objects fall back to storing the
    /// pointer bytes as the file content; when false, missing objects are a hard error.
    allow_not_found: bool,
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
        url_format: LfsServerUrlFormat,
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

        let client = Client::builder(TokioExecutor::new()).build(connector);
        let upstream = UpstreamLfs {
            lfs_server,
            url_format,
            allow_not_found,
            max_attempts,
            time_ms_between_attempts: 10000,
            conn_limit_sem: conn_limit.map(|x| Arc::new(Semaphore::new(x))),
            client,
        };
        Ok(GitImportLfs {
            inner: Some(Arc::new(GitImportLfsInner::Upstream(upstream))),
        })
    }

    /// Build a `GitImportLfs` that looks up LFS object bytes directly in the local
    /// Mononoke filestore by SHA256 alias instead of issuing HTTP requests against
    /// an upstream LFS server.
    pub fn new_internal(blobstore: Arc<dyn KeyedBlobstore>, allow_not_found: bool) -> Self {
        let internal = InternalLfs {
            blobstore,
            allow_not_found,
        };
        GitImportLfs {
            inner: Some(Arc::new(GitImportLfsInner::Internal(internal))),
        }
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
    async fn fetch_bytes_upstream(
        upstream: &UpstreamLfs,
        ctx: &CoreContext,
        metadata: &LfsPointerData,
    ) -> Result<
        (
            StoreRequest,
            Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
            GitLfsFetchResult,
        ),
        Error,
    > {
        let uri = upstream
            .url_format
            .build_object_url(&upstream.lfs_server, &metadata.sha256)?;
        let mut req = Request::get(uri.clone())
            .body(Full::new(Bytes::new()))
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
        let resp = upstream
            .client
            .request(req)
            .await
            .with_context(|| format!("fetch_bytes_upstream {}", uri))?;

        if resp.status().is_success() {
            let bytes = resp.into_body().into_data_stream().map_err(Error::from);
            let sr = StoreRequest::with_sha256(metadata.size, metadata.sha256);
            return Ok((sr, Box::new(bytes), GitLfsFetchResult::Fetched));
        }
        if resp.status() == StatusCode::NOT_FOUND && upstream.allow_not_found {
            warn!(
                "{} not found. Using gitlfs metadata as file content instead.",
                uri,
            );
            return Ok(not_found_pointer_fallback(metadata)?);
        }
        Err(format_err!("{} response {:?}", uri, resp))
    }

    /// Stream LFS object bytes directly from the local Mononoke filestore.
    async fn fetch_bytes_internal_store(
        internal: &InternalLfs,
        ctx: &CoreContext,
        metadata: &LfsPointerData,
    ) -> Result<
        (
            StoreRequest,
            Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
            GitLfsFetchResult,
        ),
        Error,
    > {
        let key = FetchKey::Aliased(Alias::Sha256(metadata.sha256));
        let fetched = filestore::fetch_with_size(internal.blobstore.clone(), ctx, &key)
            .await
            .with_context(|| {
                format!(
                    "fetch_bytes_internal_store sha256:{} size:{}",
                    metadata.sha256, metadata.size,
                )
            })?;
        match fetched {
            Some((stream, size)) => {
                if size != metadata.size {
                    return Err(format_err!(
                        "LFS object sha256:{} size mismatch: filestore has {} bytes, pointer claims {}",
                        metadata.sha256,
                        size,
                        metadata.size,
                    ));
                }
                let sr = StoreRequest::with_sha256(metadata.size, metadata.sha256);
                Ok((sr, Box::new(Box::pin(stream)), GitLfsFetchResult::Fetched))
            }
            None if internal.allow_not_found => {
                warn!(
                    "LFS object sha256:{} not found in internal filestore. \
                     Using gitlfs metadata as file content instead.",
                    metadata.sha256,
                );
                not_found_pointer_fallback(metadata)
            }
            None => Err(format_err!(
                "LFS object sha256:{} not found in internal filestore",
                metadata.sha256,
            )),
        }
    }

    async fn fetch_bytes(
        &self,
        ctx: &CoreContext,
        metadata: &LfsPointerData,
    ) -> Result<
        (
            StoreRequest,
            Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
            GitLfsFetchResult,
        ),
        Error,
    > {
        let inner = self.inner.as_ref().ok_or_else(|| {
            format_err!("GitImportLfs::fetch_bytes called on disabled GitImportLfs")
        })?;

        match inner.as_ref() {
            GitImportLfsInner::Upstream(upstream) => {
                let mut attempt: u32 = 0;
                loop {
                    let r = Self::fetch_bytes_upstream(upstream, ctx, metadata).await;
                    match r {
                        Ok(res) => return Ok(res),
                        Err(err) => {
                            if attempt >= upstream.max_attempts {
                                return Err(err);
                            }
                            attempt += 1;
                            // Sleep on average time_ms_between_attempts between attempts.
                            let sleep_time_ms =
                                rand::random_range(0..upstream.time_ms_between_attempts * 2);
                            error!(
                                "{}. Attempt {} of {} - Retrying in {} ms",
                                err, attempt, upstream.max_attempts, sleep_time_ms,
                            );
                            sleep(Duration::from_millis(sleep_time_ms.into())).await;
                        }
                    }
                }
            }
            GitImportLfsInner::Internal(internal) => {
                // No retry: the local blobstore handles its own retries internally,
                // and "not in filestore" doesn't get better by waiting.
                Self::fetch_bytes_internal_store(internal, ctx, metadata).await
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
            let inner = self
                .inner
                .as_ref()
                .ok_or_else(|| format_err!("GitImportLfs::with called on disabled GitImportLfs"))?;

            // If configured a connection limit (upstream only), grab semaphore lock enforcing it.
            let _slock = match inner.as_ref() {
                GitImportLfsInner::Upstream(upstream) => match &upstream.conn_limit_sem {
                    Some(semaphore) => Some(semaphore.clone().acquire_owned().await?),
                    None => None,
                },
                GitImportLfsInner::Internal(_) => None,
            };

            let (req, bstream, fetch_result) = self.fetch_bytes(&ctx, &metadata).await?;
            f(ctx, metadata, req, bstream, fetch_result).await
        })
        .await?
    }
}

/// Build the `(StoreRequest, stream, NotFound)` triple used when the LFS object is
/// missing but `allow_not_found` is set, so the pointer bytes themselves get stored
/// as the file content.
fn not_found_pointer_fallback(
    metadata: &LfsPointerData,
) -> Result<
    (
        StoreRequest,
        Box<dyn Stream<Item = Result<Bytes, Error>> + Send + Unpin>,
        GitLfsFetchResult,
    ),
    Error,
> {
    let bytes = Bytes::copy_from_slice(&metadata.gitblob);
    let size = metadata.gitblob.len().try_into()?;
    let git_sha1 = hash::RichGitSha1::from_bytes(
        Bytes::copy_from_slice(metadata.gitid.as_bytes()),
        "blob",
        size,
    )?;
    let sr = StoreRequest::with_git_sha1(size, git_sha1);
    Ok((
        sr,
        Box::new(stream::once(futures::future::ok(bytes))),
        GitLfsFetchResult::NotFound,
    ))
}

#[cfg(test)]
mod tests {
    use context::CoreContext;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use git_types::git_lfs::LfsPointerData;
    use memblob::KeyedMemblob;
    use mononoke_macros::mononoke;

    use super::*;

    fn sha256_fixture() -> hash::Sha256 {
        hash::Sha256::from_byte_array([0xab; 32])
    }

    /// Build a synthetic LFS pointer for `sha256`/`size` plus arbitrary `gitblob`
    /// bytes representing the in-tree pointer file (used as the fallback content
    /// when `allow_not_found` kicks in).
    fn pointer_for(sha256: hash::Sha256, size: u64, gitblob: Vec<u8>) -> LfsPointerData {
        LfsPointerData {
            version: "https://git-lfs.github.com/spec/v1".to_string(),
            sha256,
            size,
            gitblob,
            // Any valid 20-byte sha1 works; the value only matters in the
            // not-found fallback path (it gets wrapped into a RichGitSha1).
            gitid: ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            is_canonical: true,
        }
    }

    /// Drive `lfs.with(...)` and collect the streamed bytes plus the fetch result.
    async fn collect_with(
        lfs: GitImportLfs,
        ctx: CoreContext,
        pointer: LfsPointerData,
    ) -> Result<(Bytes, GitLfsFetchResult), Error> {
        lfs.with(
            ctx,
            pointer,
            |_ctx, _meta, _req, bstream, fetch_result| async move {
                let chunks: Vec<Bytes> = bstream.try_collect().await?;
                let total: usize = chunks.iter().map(|c| c.len()).sum();
                let mut buf = bytes::BytesMut::with_capacity(total);
                for chunk in chunks {
                    buf.extend_from_slice(&chunk);
                }
                Ok((buf.freeze(), fetch_result))
            },
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn internal_lfs_streams_existing_blob_from_filestore(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Seed the local filestore with some content so the SHA256 alias lands.
        let content = Bytes::from_static(b"hello internal lfs world");
        let metadata = filestore::store(
            &blobstore,
            FilestoreConfig::no_chunking_filestore(),
            &ctx,
            &StoreRequest::new(content.len() as u64),
            stream::once(futures::future::ok(content.clone())),
        )
        .await
        .expect("filestore store");

        let pointer = pointer_for(metadata.sha256, metadata.total_size, vec![]);
        let lfs = GitImportLfs::new_internal(blobstore, false /* allow_not_found */);

        let (got, fetch_result) = collect_with(lfs, ctx, pointer).await.expect("with");
        assert_eq!(got, content);
        assert!(fetch_result.is_fetched());
    }

    #[mononoke::fbinit_test]
    async fn internal_lfs_missing_blob_errors_when_not_allowed(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Nothing seeded — sha256 lookup must miss.
        let pointer = pointer_for(sha256_fixture(), 42, vec![]);
        let lfs = GitImportLfs::new_internal(blobstore, false /* allow_not_found */);

        let err = collect_with(lfs, ctx, pointer)
            .await
            .expect_err("must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not found in internal filestore"),
            "unexpected error: {msg}"
        );
    }

    #[mononoke::fbinit_test]
    async fn internal_lfs_missing_blob_falls_back_to_pointer_when_allowed(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Nothing seeded; allow_not_found=true makes the pointer bytes themselves
        // become the file content.
        let pointer_bytes =
            b"version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 42\n".to_vec();
        let expected = Bytes::from(pointer_bytes.clone());
        let pointer = pointer_for(sha256_fixture(), 42, pointer_bytes);
        let lfs = GitImportLfs::new_internal(blobstore, true /* allow_not_found */);

        let (got, fetch_result) = collect_with(lfs, ctx, pointer).await.expect("with");
        assert_eq!(got, expected);
        assert!(fetch_result.is_not_found());
    }

    #[mononoke::fbinit_test]
    async fn internal_lfs_size_mismatch_is_a_hard_error(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Seed the filestore with content of one size...
        let content = Bytes::from_static(b"24 bytes of real content");
        assert_eq!(content.len(), 24);
        let metadata = filestore::store(
            &blobstore,
            FilestoreConfig::no_chunking_filestore(),
            &ctx,
            &StoreRequest::new(content.len() as u64),
            stream::once(futures::future::ok(content.clone())),
        )
        .await
        .expect("filestore store");

        // ...but advertise a different size in the pointer. Mismatch must error
        // even when `allow_not_found` would normally rescue a miss, because we
        // *did* find content — it just doesn't match what the pointer claimed.
        let pointer = pointer_for(metadata.sha256, metadata.total_size + 1, vec![]);
        let lfs = GitImportLfs::new_internal(blobstore, true /* allow_not_found */);

        let err = collect_with(lfs, ctx, pointer)
            .await
            .expect_err("must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("size mismatch"), "unexpected error: {msg}");
    }

    #[mononoke::test]
    fn dewey_url_shape() {
        let uri = LfsServerUrlFormat::LegacyDewey
            .build_object_url("https://dewey-lfs.example.com", &sha256_fixture())
            .unwrap();
        assert_eq!(
            uri.to_string(),
            format!("https://dewey-lfs.example.com/{}", sha256_fixture()),
        );
    }

    #[mononoke::test]
    fn mononoke_git_lfs_url_shape() {
        let uri = LfsServerUrlFormat::MononokeGitLfs {
            repo_name: "myrepo".to_string(),
        }
        .build_object_url(
            "https://mononoke-git-lfs.internal.tfbnw.net",
            &sha256_fixture(),
        )
        .unwrap();
        assert_eq!(
            uri.to_string(),
            format!(
                "https://mononoke-git-lfs.internal.tfbnw.net/myrepo/download_sha256/{}",
                sha256_fixture(),
            ),
        );
    }

    #[mononoke::test]
    fn mononoke_git_lfs_url_percent_encodes_repo_name() {
        let uri = LfsServerUrlFormat::MononokeGitLfs {
            repo_name: "git/foo/bar".to_string(),
        }
        .build_object_url(
            "https://mononoke-git-lfs.internal.tfbnw.net",
            &sha256_fixture(),
        )
        .unwrap();
        assert_eq!(
            uri.to_string(),
            format!(
                "https://mononoke-git-lfs.internal.tfbnw.net/git%2Ffoo%2Fbar/download_sha256/{}",
                sha256_fixture(),
            ),
        );
    }

    #[mononoke::test]
    fn trailing_slash_in_server_url_does_not_double_up() {
        let uri = LfsServerUrlFormat::LegacyDewey
            .build_object_url("https://dewey-lfs.example.com/", &sha256_fixture())
            .unwrap();
        assert_eq!(
            uri.to_string(),
            format!("https://dewey-lfs.example.com/{}", sha256_fixture()),
        );

        let uri = LfsServerUrlFormat::MononokeGitLfs {
            repo_name: "myrepo".to_string(),
        }
        .build_object_url(
            "https://mononoke-git-lfs.internal.tfbnw.net/",
            &sha256_fixture(),
        )
        .unwrap();
        assert_eq!(
            uri.to_string(),
            format!(
                "https://mononoke-git-lfs.internal.tfbnw.net/myrepo/download_sha256/{}",
                sha256_fixture(),
            ),
        );
    }
}
