/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::fs::create_dir_all;
use std::future::ready;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use async_trait::async_trait;
use bytes::Bytes as RawBytes;
use edenapi_types::make_hash_lookup_request;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::BonsaiChangesetContent;
use edenapi_types::BookmarkEntry;
use edenapi_types::BookmarkRequest;
use edenapi_types::CloneData;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitGraphRequest;
use edenapi_types::CommitHashLookupRequest;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationRequestBatch;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitId;
use edenapi_types::CommitIdScheme;
use edenapi_types::CommitKnownResponse;
use edenapi_types::CommitLocationToHashRequest;
use edenapi_types::CommitLocationToHashRequestBatch;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitMutationsRequest;
use edenapi_types::CommitMutationsResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::CommitRevlogDataRequest;
use edenapi_types::CommitTranslateIdRequest;
use edenapi_types::CommitTranslateIdResponse;
use edenapi_types::EdenApiServerError;
use edenapi_types::EphemeralPrepareRequest;
use edenapi_types::EphemeralPrepareResponse;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileRequest;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::HgFilenodeData;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoryEntry;
use edenapi_types::HistoryRequest;
use edenapi_types::HistoryResponseChunk;
use edenapi_types::IndexableId;
use edenapi_types::LandStackRequest;
use edenapi_types::LandStackResponse;
use edenapi_types::LookupRequest;
use edenapi_types::LookupResponse;
use edenapi_types::LookupResult;
use edenapi_types::PushVar;
use edenapi_types::ServerError;
use edenapi_types::SetBookmarkRequest;
use edenapi_types::ToApi;
use edenapi_types::ToWire;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::TreeRequest;
use edenapi_types::UploadBonsaiChangesetRequest;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadHgChangesetsRequest;
use edenapi_types::UploadHgFilenodeRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokenMetadata;
use edenapi_types::UploadTokensResponse;
use edenapi_types::UploadTreeEntry;
use edenapi_types::UploadTreeRequest;
use edenapi_types::UploadTreeResponse;
use futures::future::BoxFuture;
use futures::prelude::*;
use hg_http::http_client;
use http_client::AsyncResponse;
use http_client::HttpClient;
use http_client::Request;
use itertools::Itertools;
use metrics::Counter;
use metrics::EntranceGuard;
use minibytes::Bytes;
use percent_encoding::utf8_percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::NON_ALPHANUMERIC;
use progress_model::AggregatingProgressBar;
use serde::de::DeserializeOwned;
use serde::Serialize;
use types::HgId;
use types::Key;
use url::Url;

use crate::api::EdenApi;
use crate::builder::Config;
use crate::errors::EdenApiError;
use crate::response::Response;
use crate::response::ResponseMeta;
use crate::retryable::RetryableFileAttrs;
use crate::retryable::RetryableFiles;
use crate::retryable::RetryableStreamRequest;
use crate::retryable::RetryableTrees;
use crate::types::wire::pull::PullFastForwardRequest;
use crate::types::wire::pull::PullLazyRequest;

/// All non-alphanumeric characters (except hypens, underscores, and periods)
/// found in the repo's name will be percent-encoded before being used in URLs.
const RESERVED_CHARS: &AsciiSet = &NON_ALPHANUMERIC.remove(b'_').remove(b'-').remove(b'.');

const MAX_CONCURRENT_LOOKUPS_PER_REQUEST: usize = 10000;
const MAX_CONCURRENT_UPLOAD_FILENODES_PER_REQUEST: usize = 10000;
const MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST: usize = 1000;
const MAX_CONCURRENT_FILE_UPLOADS: usize = 1000;
const MAX_CONCURRENT_HASH_LOOKUPS_PER_REQUEST: usize = 1000;
const MAX_ERROR_MSG_LEN: usize = 500;

static REQUESTS_INFLIGHT: Counter = Counter::new("edenapi.req_inflight");
static FILES_INFLIGHT: Counter = Counter::new("edenapi.files_inflight");
static FILES_ATTRS_INFLIGHT: Counter = Counter::new("edenapi.files_attrs_inflight");

mod paths {
    pub const HEALTH_CHECK: &str = "health_check";
    pub const FILES2: &str = "files2";
    pub const HISTORY: &str = "history";
    pub const TREES: &str = "trees";
    pub const COMMIT_REVLOG_DATA: &str = "commit/revlog_data";
    pub const CLONE_DATA: &str = "clone";
    pub const PULL_FAST_FORWARD: &str = "pull_fast_forward_master";
    pub const PULL_LAZY: &str = "pull_lazy";
    pub const COMMIT_LOCATION_TO_HASH: &str = "commit/location_to_hash";
    pub const COMMIT_HASH_TO_LOCATION: &str = "commit/hash_to_location";
    pub const COMMIT_HASH_LOOKUP: &str = "commit/hash_lookup";
    pub const COMMIT_GRAPH: &str = "commit/graph";
    pub const COMMIT_MUTATIONS: &str = "commit/mutations";
    pub const COMMIT_TRANSLATE_ID: &str = "commit/translate_id";
    pub const BOOKMARKS: &str = "bookmarks";
    pub const SET_BOOKMARK: &str = "bookmarks/set";
    pub const LAND_STACK: &str = "land";
    pub const LOOKUP: &str = "lookup";
    pub const UPLOAD: &str = "upload/";
    pub const UPLOAD_FILENODES: &str = "upload/filenodes";
    pub const UPLOAD_TREES: &str = "upload/trees";
    pub const UPLOAD_CHANGESETS: &str = "upload/changesets";
    pub const UPLOAD_BONSAI_CHANGESET: &str = "upload/changeset/bonsai";
    pub const EPHEMERAL_PREPARE: &str = "ephemeral/prepare";
    pub const FETCH_SNAPSHOT: &str = "snapshot";
    pub const DOWNLOAD_FILE: &str = "download/file";
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

pub struct ClientInner {
    config: Config,
    client: HttpClient,
    tree_progress: Arc<AggregatingProgressBar>,
    file_progress: Arc<AggregatingProgressBar>,
}

impl Client {
    /// Create an EdenAPI client with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        let client = http_client("edenapi", config.http_config.clone());
        let inner = Arc::new(ClientInner {
            config,
            client,
            tree_progress: AggregatingProgressBar::new("fetching", "trees"),
            file_progress: AggregatingProgressBar::new("fetching", "files"),
        });
        Self { inner }
    }

    pub(crate) fn config(&self) -> &Config {
        &self.inner.config
    }

    fn repo_name(&self) -> &str {
        &self.config().repo_name
    }

    /// Append endpoint path onto the server's base URL.
    fn build_url_repoless(&self, path: &str) -> Result<Url, EdenApiError> {
        let url = &self.config().server_url;
        Ok(url.join(path)?)
    }

    /// Append a repo name and endpoint path onto the server's base URL.
    fn build_url(&self, path: &str) -> Result<Url, EdenApiError> {
        let url = &self.config().server_url;
        // Repo name must be sanitized since it can be set by the user.
        let url = url
            .join(&format!(
                "{}/",
                utf8_percent_encode(self.repo_name(), RESERVED_CHARS)
            ))?
            .join(path)?;
        Ok(url)
    }

    /// Add configured values to a request.
    fn configure_request(&self, mut req: Request) -> Result<Request, EdenApiError> {
        // This method should probably not exist. Request
        // configuration should flow through a shared config (i.e.
        // http_client::Config) that is applied by the HttpClient.
        // This way, every use of HttpClient does not its own http
        // config and glue code to apply the config to the request.

        let config = self.config();

        for (k, v) in &config.headers {
            req.set_header(k, v);
        }

        if let Some(ref correlator) = config.correlator {
            req.set_header("X-Client-Correlator", correlator);
        }

        if let Some(timeout) = config.timeout {
            req.set_timeout(timeout);
        }

        if let Some(http_version) = config.http_version {
            req.set_http_version(http_version);
        }

        if let Some(encoding) = &config.encoding {
            req.set_accept_encoding([encoding.clone()]);
        }

        if let Some(mts) = &config.min_transfer_speed {
            req.set_min_transfer_speed(*mts);
        }

        Ok(req)
    }

    /// Prepare a collection of POST requests for the given keys.
    /// The keys will be grouped into batches of the specified size and
    /// passed to the `make_req` callback, which should insert them into
    /// a struct that will be CBOR-encoded and used as the request body.
    fn prepare_requests<T, K, F, R>(
        &self,
        url: &Url,
        keys: K,
        batch_size: Option<usize>,
        mut make_req: F,
    ) -> Result<Vec<Request>, EdenApiError>
    where
        K: IntoIterator<Item = T>,
        F: FnMut(Vec<T>) -> R,
        R: ToWire,
    {
        split_into_batches(keys, batch_size)
            .into_iter()
            .map(|keys| {
                let req = make_req(keys).to_wire();
                self.configure_request(self.inner.client.post(url.clone()))?
                    .cbor(&req)
                    .map_err(EdenApiError::RequestSerializationFailed)
            })
            .collect()
    }

    /// Fetch data from the server without Wire to Api conversion.
    ///
    /// Concurrently performs all of the given HTTP requests, each of
    /// which must result in streaming response of CBOR-encoded values
    /// of type `T`. The metadata of each response will be returned in
    /// the order the responses arrive. The response streams will be
    /// combined into a single stream, in which the returned entries
    /// from different HTTP responses may be arbitrarily interleaved.
    fn fetch_raw<T: DeserializeOwned + Send + 'static>(
        &self,
        requests: Vec<Request>,
    ) -> Result<Response<T>, EdenApiError> {
        let (responses, stats) = self.inner.client.send_async(requests)?;

        // Transform each response `Future` (which resolves when all of the HTTP
        // headers for that response have been received) into a `Stream` that
        // waits until all headers have been received and then starts yielding
        // entries. This allows multiplexing the streams using `select_all`.
        let streams = responses.into_iter().map(|fut| {
            stream::once(async move {
                let res = raise_for_status(fut.await?).await?;
                tracing::debug!("{:?}", ResponseMeta::from(&res));
                Ok::<_, EdenApiError>(res.into_body().cbor::<T>().err_into())
            })
            .try_flatten()
            .boxed()
        });

        let entries = stream::select_all(streams).boxed();
        let stats = stats.err_into().boxed();

        Ok(Response { entries, stats })
    }

    /// Fetch data from the server.
    ///
    /// Concurrently performs all of the given HTTP requests, each of
    /// which must result in streaming response of CBOR-encoded values
    /// of type `T`. The metadata of each response will be returned in
    /// the order the responses arrive. The response streams will be
    /// combined into a single stream, in which the returned entries
    /// from different HTTP responses may be arbitrarily interleaved.
    fn fetch<T>(&self, requests: Vec<Request>) -> Result<Response<T>, EdenApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.fetch_guard::<T>(requests, vec![])
    }

    fn fetch_guard<T>(
        &self,
        requests: Vec<Request>,
        mut guards: Vec<EntranceGuard>,
    ) -> Result<Response<T>, EdenApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        guards.push(REQUESTS_INFLIGHT.entrance_guard(requests.len()));
        let Response { entries, stats } = self.fetch_raw::<<T as ToWire>::Wire>(requests)?;

        let stats = metrics::wrap_future_keep_guards(stats, guards).boxed();
        let entries = entries
            .and_then(|v| future::ready(v.to_api().map_err(|e| EdenApiError::from(e.into()))))
            .boxed();

        Ok(Response { entries, stats })
    }

    /// Similar to `fetch`. But returns a `Vec` directly.
    async fn fetch_vec<T>(&self, requests: Vec<Request>) -> Result<Vec<T>, EdenApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.fetch::<T>(requests)?.flatten().await
    }

    /// Similar to `fetch_vec`. But with retries.
    async fn fetch_vec_with_retry<T>(&self, requests: Vec<Request>) -> Result<Vec<T>, EdenApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.with_retry(|this| this.fetch_vec::<T>(requests.clone()).boxed())
            .await
    }

    /// Similar to `fetch`, but returns the response type directly, instead of Response<_>.
    async fn fetch_single<T>(&self, request: Request) -> Result<T, EdenApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.fetch::<T>(vec![request])?.single().await
    }

    /// Log the request to the configured log directory as JSON.
    fn log_request<R: Serialize + Debug>(&self, req: &R, label: &str) {
        tracing::trace!("Sending request: {:?}", req);

        let log_dir = match &self.config().log_dir {
            Some(path) => path.clone(),
            None => return,
        };

        let value: serde_cbor::Value = match serde_cbor::value::to_value(req) {
            Ok(v) => v,
            Err(_e) => return,
        };
        let timestamp = chrono::Local::now().format("%y%m%d_%H%M%S_%f");
        let name = format!("{}_{}.log", &timestamp, label);
        let path = log_dir.join(&name);

        let _ = async_runtime::spawn_blocking(move || {
            if let Err(e) = || -> std::io::Result<()> {
                create_dir_all(&log_dir)?;
                let data = pprint::pformat_value(&value);
                std::fs::write(&path, data)
            }() {
                tracing::warn!("Failed to log request: {:?}", &e);
            }
        });
    }

    pub(crate) async fn fetch_files(
        &self,
        keys: Vec<Key>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        tracing::info!("Requesting content for {} file(s)", keys.len());

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let guards = vec![FILES_INFLIGHT.entrance_guard(keys.len())];

        let url = self.build_url(paths::FILES2)?;
        let requests = self.prepare_requests(&url, keys, self.config().max_files, |keys| {
            let req = FileRequest { keys, reqs: vec![] };
            self.log_request(&req, "files");
            req
        })?;

        Ok(self.fetch_guard::<FileResponse>(requests, guards)?)
    }

    pub(crate) async fn fetch_trees(
        &self,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        tracing::info!("Requesting {} tree(s)", keys.len());

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::TREES)?;
        let requests = self.prepare_requests(&url, keys, self.config().max_trees, |keys| {
            let req = TreeRequest {
                keys,
                attributes: attributes.clone().unwrap_or_default(),
            };
            self.log_request(&req, "trees");
            req
        })?;

        Ok(self.fetch::<Result<TreeEntry, EdenApiServerError>>(requests)?)
    }

    pub(crate) async fn fetch_files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        tracing::info!("Requesting attributes for {} file(s)", reqs.len());

        if reqs.is_empty() {
            return Ok(Response::empty());
        }

        let guards = vec![FILES_ATTRS_INFLIGHT.entrance_guard(reqs.len())];

        let url = self.build_url(paths::FILES2)?;
        let requests = self.prepare_requests(&url, reqs, self.config().max_files, |reqs| {
            let req = FileRequest { reqs, keys: vec![] };
            self.log_request(&req, "files");
            req
        })?;

        Ok(self.fetch_guard::<FileResponse>(requests, guards)?)
    }

    /// Upload a single file
    async fn process_single_file_upload(
        &self,
        item: AnyFileContentId,
        raw_content: Bytes,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadToken>, EdenApiError> {
        let mut url = self.build_url(paths::UPLOAD)?;
        url = url.join("file/")?;
        match item {
            AnyFileContentId::ContentId(id) => {
                url = url.join("content_id/")?.join(&format!("{}", id))?;
            }
            AnyFileContentId::Sha1(id) => {
                url = url.join("sha1/")?.join(&format!("{}", id))?;
            }
            AnyFileContentId::Sha256(id) => {
                url = url.join("sha256/")?.join(&format!("{}", id))?;
            }
        }

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("content_size", &raw_content.len().to_string());
            if let Some(bubble_id) = bubble_id {
                query.append_pair("bubble_id", &bubble_id.to_string());
            }
        }

        let msg = format!("Requesting upload for {}", url);
        tracing::info!("{}", &msg);

        Ok(self.fetch::<UploadToken>(vec![{
            self.configure_request(self.inner.client.put(url.clone()))?
                .body(raw_content.to_vec())
        }])?)
    }

    async fn clone_data_attempt(&self) -> Result<CloneData<HgId>, EdenApiError> {
        let url = self.build_url(paths::CLONE_DATA)?;
        let req = self.configure_request(self.inner.client.post(url))?;
        let mut fetch = self.fetch::<CloneData<HgId>>(vec![req])?;
        fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })?
    }

    async fn pull_lazy_attempt(
        &self,
        req: PullLazyRequest,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let url = self.build_url(paths::PULL_LAZY)?;
        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;
        let mut fetch = self.fetch::<CloneData<HgId>>(vec![req])?;
        fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })?
    }

    async fn fast_forward_pull_attempt(
        &self,
        req: PullFastForwardRequest,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let url = self.build_url(paths::PULL_FAST_FORWARD)?;
        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;
        let mut fetch = self.fetch::<CloneData<HgId>>(vec![req])?;
        fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })?
    }

    async fn with_retry<'t, T>(
        &'t self,
        func: impl Fn(&'t Self) -> BoxFuture<'t, Result<T, EdenApiError>>,
    ) -> Result<T, EdenApiError> {
        let retry_count = self.inner.config.max_retry_per_request;
        with_retry(retry_count, || func(self)).await
    }
}

#[async_trait]
impl EdenApi for Client {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        let url = self.build_url_repoless(paths::HEALTH_CHECK)?;

        tracing::info!("Sending health check request: {}", &url);

        let req = self.configure_request(self.inner.client.get(url))?;
        let res = raise_for_status(req.send_async().await?).await?;

        Ok(ResponseMeta::from(&res))
    }

    async fn capabilities(&self) -> Result<Vec<String>, EdenApiError> {
        tracing::info!("Requesting capabilities for repo {}", &self.repo_name());
        let url = self.build_url("capabilities")?;
        let req = self.configure_request(self.inner.client.get(url))?;
        let res = raise_for_status(req.send_async().await?).await?;
        let body: Vec<u8> = res.into_body().decoded().try_concat().await?;
        let caps = serde_json::from_slice(&body)
            .map_err(|e| EdenApiError::ParseResponse(e.to_string()))?;
        Ok(caps)
    }

    async fn files(&self, keys: Vec<Key>) -> Result<Response<FileResponse>, EdenApiError> {
        tracing::info!("Requesting content for {} file(s)", keys.len());

        let prog = self.inner.file_progress.create_or_extend(keys.len() as u64);

        RetryableFiles::new(keys)
            .perform_with_retries(self.clone())
            .and_then(|r| async {
                Ok(r.then(move |r| {
                    prog.increase_position(1);
                    ready(r)
                }))
            })
            .await
    }

    async fn files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        tracing::info!("Requesting attributes for {} file(s)", reqs.len());

        let prog = self.inner.file_progress.create_or_extend(reqs.len() as u64);

        RetryableFileAttrs::new(reqs)
            .perform_with_retries(self.clone())
            .and_then(|r| async {
                Ok(r.then(move |r| {
                    prog.increase_position(1);
                    ready(r)
                }))
            })
            .await
    }

    async fn history(
        &self,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, EdenApiError> {
        tracing::info!("Requesting history for {} file(s)", keys.len());

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::HISTORY)?;
        let requests = self.prepare_requests(&url, keys, self.config().max_history, |keys| {
            let req = HistoryRequest { keys, length };
            self.log_request(&req, "history");
            req
        })?;

        let Response { entries, stats } = self.fetch::<HistoryResponseChunk>(requests)?;

        // Convert received `HistoryResponseChunk`s into `HistoryEntry`s.
        let entries = entries
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten()
            .boxed();

        Ok(Response { entries, stats })
    }

    async fn trees(
        &self,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        tracing::info!("Requesting {} tree(s)", keys.len());

        let prog = self.inner.tree_progress.create_or_extend(keys.len() as u64);

        RetryableTrees::new(keys, attributes)
            .perform_with_retries(self.clone())
            .and_then(|r| async {
                Ok(r.then(move |r| {
                    prog.increase_position(1);
                    ready(r)
                }))
            })
            .await
    }

    async fn commit_revlog_data(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitRevlogData>, EdenApiError> {
        tracing::info!("Requesting revlog data for {} commit(s)", hgids.len());

        let url = self.build_url(paths::COMMIT_REVLOG_DATA)?;
        let commit_revlog_data_req = CommitRevlogDataRequest { hgids };

        self.log_request(&commit_revlog_data_req, "commit_revlog_data");

        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&commit_revlog_data_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_raw::<CommitRevlogData>(vec![req])
    }

    async fn hash_prefixes_lookup(
        &self,
        prefixes: Vec<String>,
    ) -> Result<Vec<CommitHashLookupResponse>, EdenApiError> {
        tracing::info!("Requesting full hashes for {} prefix(es)", prefixes.len());
        let url = self.build_url(paths::COMMIT_HASH_LOOKUP)?;
        let prefixes: Vec<CommitHashLookupRequest> = prefixes
            .into_iter()
            .map(|prefix| make_hash_lookup_request(prefix))
            .collect::<Result<Vec<CommitHashLookupRequest>, _>>()?;
        let requests = self.prepare_requests(
            &url,
            prefixes,
            Some(MAX_CONCURRENT_HASH_LOOKUPS_PER_REQUEST),
            |prefixes| Batch::<_> { batch: prefixes },
        )?;
        self.fetch_vec_with_retry::<CommitHashLookupResponse>(requests)
            .await
    }

    async fn bookmarks(&self, bookmarks: Vec<String>) -> Result<Vec<BookmarkEntry>, EdenApiError> {
        tracing::info!("Requesting '{}' bookmarks", bookmarks.len());
        let url = self.build_url(paths::BOOKMARKS)?;
        let bookmark_req = BookmarkRequest { bookmarks };
        self.log_request(&bookmark_req, "bookmarks");
        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&bookmark_req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_vec_with_retry::<BookmarkEntry>(vec![req]).await
    }

    async fn set_bookmark(
        &self,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: HashMap<String, String>,
    ) -> Result<(), EdenApiError> {
        tracing::info!("Set bookmark '{}' from {:?} to {:?}", &bookmark, from, to);
        let url = self.build_url(paths::SET_BOOKMARK)?;
        let set_bookmark_req = SetBookmarkRequest {
            bookmark,
            to,
            from,
            pushvars: pushvars
                .into_iter()
                .map(|(k, v)| PushVar { key: k, value: v })
                .collect(),
        };
        self.log_request(&set_bookmark_req, "set_bookmark");
        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&set_bookmark_req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_single::<()>(req).await
    }

    /// Land a stack of commits, rebasing them onto the specified bookmark
    /// and updating the bookmark to the top of the rebased stack
    async fn land_stack(
        &self,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> Result<LandStackResponse, EdenApiError> {
        tracing::info!(
            "Landing stack between head {} and base {} to bookmark '{}'",
            head,
            base,
            &bookmark
        );
        let url = self.build_url(paths::LAND_STACK)?;

        let land_stack_req = LandStackRequest {
            bookmark,
            head,
            base,
            pushvars: pushvars
                .into_iter()
                .map(|(k, v)| PushVar { key: k, value: v })
                .collect(),
        };
        self.log_request(&land_stack_req, "land");
        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&land_stack_req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_single::<LandStackResponse>(req).await
    }

    async fn clone_data(&self) -> Result<CloneData<HgId>, EdenApiError> {
        tracing::info!(
            "Requesting clone data for the '{}' repository",
            self.repo_name(),
        );
        self.with_retry(|this| this.clone_data_attempt().boxed())
            .await
    }

    async fn pull_fast_forward_master(
        &self,
        old_master: HgId,
        new_master: HgId,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        tracing::info!(
            "Requesting pull fast forward data for the '{}' repository",
            self.repo_name()
        );

        self.with_retry(|this| {
            let req = PullFastForwardRequest {
                old_master,
                new_master,
            };
            this.fast_forward_pull_attempt(req).boxed()
        })
        .await
    }

    async fn pull_lazy(
        &self,
        common: Vec<HgId>,
        missing: Vec<HgId>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        tracing::info!(
            "Requesting pull lazy data for the '{}' repository",
            self.repo_name()
        );

        self.with_retry(|this| {
            let req = PullLazyRequest {
                common: common.clone(),
                missing: missing.clone(),
            };
            this.pull_lazy_attempt(req).boxed()
        })
        .await
    }

    async fn commit_location_to_hash(
        &self,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> Result<Vec<CommitLocationToHashResponse>, EdenApiError> {
        tracing::info!(
            "Requesting commit location to hash (batch size = {})",
            requests.len()
        );
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let url = self.build_url(paths::COMMIT_LOCATION_TO_HASH)?;

        let formatted = self.prepare_requests(
            &url,
            requests,
            self.config().max_location_to_hash,
            |requests| {
                let batch = CommitLocationToHashRequestBatch { requests };
                self.log_request(&batch, "commit_location_to_hash");
                batch
            },
        )?;

        self.fetch_vec_with_retry::<CommitLocationToHashResponse>(formatted)
            .await
    }

    async fn commit_hash_to_location(
        &self,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitHashToLocationResponse>, EdenApiError> {
        tracing::info!(
            "Requesting commit hash to location (batch size = {})",
            hgids.len()
        );

        if hgids.is_empty() {
            return Ok(Vec::new());
        }

        let url = self.build_url(paths::COMMIT_HASH_TO_LOCATION)?;

        let formatted =
            self.prepare_requests(&url, hgids, self.config().max_location_to_hash, |hgids| {
                let batch = CommitHashToLocationRequestBatch {
                    master_heads: master_heads.clone(),
                    hgids,
                    unfiltered: Some(true),
                };
                self.log_request(&batch, "commit_hash_to_location");
                batch
            })?;

        self.fetch_vec_with_retry::<CommitHashToLocationResponse>(formatted)
            .await
    }

    async fn commit_known(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitKnownResponse>, EdenApiError> {
        let anyids: Vec<_> = hgids
            .iter()
            .cloned()
            .map(|hgid| AnyId::HgChangesetId(hgid))
            .collect();
        let entries = self.lookup_batch(anyids.clone(), None, None).await?;

        let into_hgid = |id: IndexableId| match id.id {
            AnyId::HgChangesetId(hgid) => Ok(hgid),
            _ => Err(EdenApiError::Other(format_err!("Invalid id returned"))),
        };

        let id_to_token: HashMap<HgId, Option<UploadToken>> = entries
            .into_iter()
            .map(|response| match response.result {
                LookupResult::NotPresent(id) => Ok((into_hgid(id)?, None)),
                LookupResult::Present(token) => Ok((into_hgid(token.indexable_id())?, Some(token))),
            })
            .collect::<Result<_, EdenApiError>>()?;

        Ok(hgids
            .into_iter()
            .map(|hgid| match id_to_token.get(&hgid) {
                Some(value) => CommitKnownResponse {
                    hgid,
                    known: Ok(value.is_some()),
                },
                None => CommitKnownResponse {
                    hgid,
                    known: Err(ServerError::generic(
                        "the server cannot check `HgChangesetId`",
                    )),
                },
            })
            .collect::<Vec<CommitKnownResponse>>())
    }

    async fn commit_graph(
        &self,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Vec<CommitGraphEntry>, EdenApiError> {
        tracing::info!(
            "Requesting commit graph with {} heads and {} common",
            heads.len(),
            common.len()
        );
        let url = self.build_url(paths::COMMIT_GRAPH)?;
        let graph_req = CommitGraphRequest { heads, common };
        self.log_request(&graph_req, "commit_graph");
        let wire_graph_req = graph_req.to_wire();

        let req = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&wire_graph_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_vec_with_retry::<CommitGraphEntry>(vec![req])
            .await
    }

    async fn lookup_batch(
        &self,
        items: Vec<AnyId>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Vec<LookupResponse>, EdenApiError> {
        tracing::info!("Requesting lookup for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let url = self.build_url(paths::LOOKUP)?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_LOOKUPS_PER_REQUEST),
            |ids| Batch::<LookupRequest> {
                batch: ids
                    .into_iter()
                    .map(|id| LookupRequest {
                        id,
                        bubble_id,
                        copy_from_bubble_id,
                    })
                    .collect(),
            },
        )?;

        self.fetch_vec_with_retry::<LookupResponse>(requests).await
    }

    async fn process_files_upload(
        &self,
        data: Vec<(AnyFileContentId, Bytes)>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadToken>, EdenApiError> {
        if data.is_empty() {
            return Ok(Response::empty());
        }
        // Filter already uploaded file contents first

        let mut uploaded_ids = HashSet::new();
        let mut uploaded_tokens: Vec<UploadToken> = vec![];

        let anyids: Vec<_> = data
            .iter()
            .map(|(id, _data)| AnyId::AnyFileContentId(id.clone()))
            .collect();

        let entries = self
            .lookup_batch(anyids.clone(), bubble_id, copy_from_bubble_id)
            .await?;
        for entry in entries {
            if let LookupResult::Present(token) = entry.result {
                uploaded_ids.insert(token.indexable_id());
                uploaded_tokens.push(token);
            }
        }

        let msg = format!(
            "Received {} token(s) from the lookup_batch request",
            uploaded_tokens.len()
        );
        tracing::info!("{}", &msg);

        // Upload the rest of the contents in parallel
        let new_tokens = stream::iter(
            data.into_iter()
                .filter(|(id, _content)| {
                    !uploaded_ids.contains(&IndexableId {
                        id: AnyId::AnyFileContentId(id.clone()),
                        bubble_id,
                    })
                })
                .map(|(id, content)| async move {
                    self.process_single_file_upload(id, content, bubble_id)
                        .await?
                        .entries
                        .next()
                        .await
                        .ok_or_else(|| {
                            EdenApiError::Other(format_err!(
                                "token data is missing from the reponse body for {}",
                                id
                            ))
                        })?
                }),
        )
        .buffer_unordered(MAX_CONCURRENT_FILE_UPLOADS)
        .collect::<Vec<_>>()
        .await;

        let msg = format!(
            "Received {} new token(s) from upload requests",
            new_tokens.iter().filter(|x| x.is_ok()).count()
        );
        tracing::info!("{}", &msg);

        // Merge all the tokens together
        let all_tokens = new_tokens
            .into_iter()
            .chain(uploaded_tokens.into_iter().map(|token| Ok(token)))
            .collect::<Vec<Result<_, _>>>();

        Ok(Response {
            stats: Box::pin(async { Ok(Default::default()) }),
            entries: Box::pin(futures::stream::iter(all_tokens)),
        })
    }

    async fn upload_filenodes_batch(
        &self,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        tracing::info!("Requesting hg filenodes upload for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_FILENODES)?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_UPLOAD_FILENODES_PER_REQUEST),
            |ids| Batch::<_> {
                batch: ids
                    .into_iter()
                    .map(|item| UploadHgFilenodeRequest { data: item })
                    .collect(),
            },
        )?;

        Ok(self.fetch::<UploadTokensResponse>(requests)?)
    }

    async fn upload_trees_batch(
        &self,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, EdenApiError> {
        tracing::info!("Requesting trees upload for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_TREES)?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST),
            |ids| Batch::<_> {
                batch: ids
                    .into_iter()
                    .map(|item| UploadTreeRequest { entry: item })
                    .collect(),
            },
        )?;

        Ok(self.fetch::<UploadTreeResponse>(requests)?)
    }

    // the request isn't batched, batching should be done outside if needed
    async fn upload_changesets(
        &self,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        tracing::info!(
            "Requesting changesets upload for {} item(s)",
            changesets.len(),
        );

        if changesets.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_CHANGESETS)?;
        let req = UploadHgChangesetsRequest {
            changesets,
            mutations,
        }
        .to_wire();

        let request = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<UploadTokensResponse>(vec![request])?)
    }

    async fn upload_bonsai_changeset(
        &self,
        changeset: BonsaiChangesetContent,
        bubble_id: Option<std::num::NonZeroU64>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        tracing::info!("Requesting changeset upload");

        let mut url = self.build_url(paths::UPLOAD_BONSAI_CHANGESET)?;
        if let Some(bubble_id) = bubble_id {
            url.query_pairs_mut()
                .append_pair("bubble_id", &bubble_id.to_string());
        }
        let req = UploadBonsaiChangesetRequest { changeset }.to_wire();

        let request = self
            .configure_request(self.inner.client.post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<UploadTokensResponse>(vec![request])?)
    }

    async fn ephemeral_prepare(
        &self,
        custom_duration: Option<Duration>,
    ) -> Result<Response<EphemeralPrepareResponse>, EdenApiError> {
        tracing::info!("Preparing ephemeral bubble");
        let url = self.build_url(paths::EPHEMERAL_PREPARE)?;
        let req = EphemeralPrepareRequest {
            custom_duration_secs: custom_duration.map(|d| d.as_secs()),
        }
        .to_wire();
        let request = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        let mut fetch = self.fetch::<EphemeralPrepareResponse>(vec![request])?;
        fetch.entries = fetch
            .entries
            .inspect_ok(|r| tracing::info!("Created bubble {}", r.bubble_id))
            .boxed();

        Ok(fetch)
    }

    async fn fetch_snapshot(
        &self,
        request: FetchSnapshotRequest,
    ) -> Result<Response<FetchSnapshotResponse>, EdenApiError> {
        tracing::info!("Fetching snapshot {}", request.cs_id,);
        let url = self.build_url(paths::FETCH_SNAPSHOT)?;
        let req = request.to_wire();
        let request = self
            .configure_request(self.inner.client.post(url))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<FetchSnapshotResponse>(vec![request])?)
    }

    async fn download_file(&self, token: UploadToken) -> Result<Bytes, EdenApiError> {
        tracing::info!("Downloading file");
        let url = self.build_url(paths::DOWNLOAD_FILE)?;
        let metadata = token.data.metadata.clone();
        let req = token.to_wire();
        let request = self
            .configure_request(self.inner.client.post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        use bytes::BytesMut;
        let buf = if let Some(UploadTokenMetadata::FileContentTokenMetadata(m)) = metadata {
            BytesMut::with_capacity(m.content_size.try_into().unwrap_or_default())
        } else {
            BytesMut::new()
        };

        Ok(self
            .fetch::<RawBytes>(vec![request])?
            .entries
            .try_fold(buf, |mut buf, chunk| async move {
                buf.extend_from_slice(&chunk);
                Ok(buf)
            })
            .await?
            .freeze()
            .into())
    }

    async fn commit_mutations(
        &self,
        commits: Vec<HgId>,
    ) -> Result<Vec<CommitMutationsResponse>, EdenApiError> {
        tracing::info!("Requesting mutation info for {} commits", commits.len());
        let url = self.build_url(paths::COMMIT_MUTATIONS)?;
        let requests = self.prepare_requests(
            &url,
            commits,
            self.config().max_commit_mutations,
            |commits| {
                let req = CommitMutationsRequest { commits };
                self.log_request(&req, "commit_mutations");
                req
            },
        )?;

        self.fetch_vec_with_retry::<CommitMutationsResponse>(requests)
            .await
    }

    async fn commit_translate_id(
        &self,
        commits: Vec<CommitId>,
        scheme: CommitIdScheme,
    ) -> Result<Response<CommitTranslateIdResponse>, EdenApiError> {
        tracing::info!(
            "Requesting commit id translation for {} commits into {:?}",
            commits.len(),
            scheme
        );
        let url = self.build_url(paths::COMMIT_TRANSLATE_ID)?;
        let requests = self.prepare_requests(
            &url,
            commits,
            self.config().max_commit_translate_id,
            |commits| {
                let req = CommitTranslateIdRequest { commits, scheme };
                self.log_request(&req, "commit_translate_id");
                req
            },
        )?;
        Ok(self.fetch::<CommitTranslateIdResponse>(requests)?)
    }
}

/// Split up a collection of keys into batches of at most `batch_size`.
fn split_into_batches<T>(
    keys: impl IntoIterator<Item = T>,
    batch_size: Option<usize>,
) -> Vec<Vec<T>> {
    match batch_size {
        Some(n) => keys
            .into_iter()
            .chunks(n)
            .into_iter()
            .map(Vec::from_iter)
            .collect(),
        None => vec![keys.into_iter().collect()],
    }
}

async fn raise_for_status(res: AsyncResponse) -> Result<AsyncResponse, EdenApiError> {
    let status = res.status();
    if status.as_u16() < 400 {
        return Ok(res);
    }

    let url = res.url().to_string();
    let (head, body) = res.into_parts();
    let body = body.decoded().try_concat().await?;
    let mut message = String::from_utf8_lossy(&body).into_owned();

    if message.len() >= 9 && &*message[..9].to_lowercase() == "<!doctype" {
        message = "HTML content omitted (this error may have come from a proxy server)".into();
    } else if message.len() > MAX_ERROR_MSG_LEN {
        message.truncate(MAX_ERROR_MSG_LEN);
        message.push_str("... (truncated)")
    }

    let headers = head.headers().clone();
    Err(EdenApiError::HttpError {
        status,
        message,
        headers,
        url,
    })
}

async fn with_retry<'t, T>(
    max_retry_count: usize,
    func: impl Fn() -> BoxFuture<'t, Result<T, EdenApiError>>,
) -> Result<T, EdenApiError> {
    let mut attempt = 0usize;
    loop {
        let result = func().await;
        if attempt >= max_retry_count {
            return result;
        }
        match result {
            Ok(result) => return Ok(result),
            Err(ref error) => {
                if !error.is_retryable() {
                    return result;
                }
                tracing::warn!("Retrying http error {:?}", error);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::builder::HttpClientBuilder;

    #[test]
    fn test_url_escaping() -> Result<()> {
        let base_url = "https://example.com".parse()?;
        let repo_name = "repo_-. !@#$% foo \u{1f4a9} bar";

        let client = HttpClientBuilder::new()
            .repo_name(repo_name)
            .server_url(base_url)
            .build()?;

        let path = "path";
        let url: String = client.build_url(path)?.into();
        let expected =
            "https://example.com/repo_-.%20%21%40%23%24%25%20foo%20%F0%9F%92%A9%20bar/path";
        assert_eq!(&url, &expected);

        Ok(())
    }
}
