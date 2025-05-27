/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::fs::create_dir_all;
use std::future::ready;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::format_err;
use async_trait::async_trait;
use edenapi_types::AlterSnapshotRequest;
use edenapi_types::AlterSnapshotResponse;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::BlameRequest;
use edenapi_types::BlameResult;
use edenapi_types::BonsaiChangesetContent;
use edenapi_types::BookmarkEntry;
use edenapi_types::BookmarkRequest;
use edenapi_types::BookmarkResult;
use edenapi_types::CloudShareWorkspaceRequest;
use edenapi_types::CloudShareWorkspaceResponse;
use edenapi_types::CloudWorkspaceRequest;
use edenapi_types::CloudWorkspacesRequest;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitGraphRequest;
use edenapi_types::CommitGraphSegmentsEntry;
use edenapi_types::CommitGraphSegmentsRequest;
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
use edenapi_types::EphemeralPrepareRequest;
use edenapi_types::EphemeralPrepareResponse;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileRequest;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogByVersionParams;
use edenapi_types::GetSmartlogParams;
use edenapi_types::HgFilenodeData;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoricalVersionsParams;
use edenapi_types::HistoricalVersionsResponse;
use edenapi_types::HistoryEntry;
use edenapi_types::HistoryRequest;
use edenapi_types::HistoryResponseChunk;
use edenapi_types::IdenticalChangesetContent;
use edenapi_types::IndexableId;
use edenapi_types::LandStackRequest;
use edenapi_types::LandStackResponse;
use edenapi_types::LookupRequest;
use edenapi_types::LookupResponse;
use edenapi_types::LookupResult;
use edenapi_types::PathHistoryRequest;
use edenapi_types::PathHistoryRequestPaginationCursor;
use edenapi_types::PathHistoryResponse;
use edenapi_types::PushVar;
use edenapi_types::ReferencesDataResponse;
use edenapi_types::RenameWorkspaceRequest;
use edenapi_types::RenameWorkspaceResponse;
use edenapi_types::RepoPathBuf;
use edenapi_types::RollbackWorkspaceRequest;
use edenapi_types::RollbackWorkspaceResponse;
use edenapi_types::SaplingRemoteApiServerError;
use edenapi_types::ServerError;
use edenapi_types::SetBookmarkRequest;
use edenapi_types::SetBookmarkResponse;
use edenapi_types::SuffixQueryRequest;
use edenapi_types::SuffixQueryResponse;
use edenapi_types::ToApi;
use edenapi_types::ToWire;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::TreeRequest;
use edenapi_types::UpdateArchiveParams;
use edenapi_types::UpdateArchiveResponse;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::UploadBonsaiChangesetRequest;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadHgChangesetsRequest;
use edenapi_types::UploadHgFilenodeRequest;
use edenapi_types::UploadIdenticalChangesetsRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokenMetadata;
use edenapi_types::UploadTokensResponse;
use edenapi_types::UploadTreeEntry;
use edenapi_types::UploadTreeRequest;
use edenapi_types::UploadTreeResponse;
use edenapi_types::WorkspaceDataResponse;
use edenapi_types::WorkspacesDataResponse;
use edenapi_types::bookmark::Bookmark2Request;
use edenapi_types::bookmark::Freshness;
use edenapi_types::cloud::SmartlogDataResponse;
use edenapi_types::make_hash_lookup_request;
use futures::future::BoxFuture;
use futures::prelude::*;
use hg_http::http_client;
use http_client::AsyncResponse;
use http_client::Encoding;
use http_client::HttpClient;
use http_client::Request;
use itertools::Itertools;
use metrics::Counter;
use metrics::EntranceGuard;
use minibytes::Bytes as RawBytes;
use minibytes::Bytes;
use once_cell::sync::Lazy;
use parking_lot::Once;
use progress_model::ProgressBar;
use repourl::encode_repo_name;
use serde::Serialize;
use serde::de::DeserializeOwned;
use types::FetchContext;
use types::HgId;
use types::Key;
use url::Url;

use crate::api::SaplingRemoteApi;
use crate::api::UploadLookupPolicy;
use crate::builder::Config;
use crate::errors::SaplingRemoteApiError;
use crate::response::Response;
use crate::response::ResponseMeta;
use crate::retryable::RetryableFileAttrs;
use crate::retryable::RetryableStreamRequest;
use crate::retryable::RetryableTrees;

const MAX_CONCURRENT_LOOKUPS_PER_REQUEST: usize = 10000;
const MAX_CONCURRENT_UPLOAD_FILENODES_PER_REQUEST: usize = 10000;
const MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST: usize = 1000;
const MAX_CONCURRENT_FILE_UPLOADS: usize = 1000;
const MAX_CONCURRENT_HASH_LOOKUPS_PER_REQUEST: usize = 1000;
const MAX_CONCURRENT_BLAMES_PER_REQUEST: usize = 10;
const MAX_ERROR_MSG_LEN: usize = 500;

static REQUESTS_INFLIGHT: Counter = Counter::new_counter("edenapi.req_inflight");
static FILES_ATTRS_INFLIGHT: Counter = Counter::new_counter("edenapi.files_attrs_inflight");

pub static RECENT_DOGFOODING_REQUESTS: Lazy<ExpiringBool> =
    Lazy::new(|| ExpiringBool::new(Duration::from_secs(5)));

pub mod paths {
    pub const ALTER_SNAPSHOT: &str = "snapshot/alter";
    pub const BLAME: &str = "blame";
    pub const BOOKMARKS: &str = "bookmarks";
    pub const BOOKMARKS2: &str = "bookmarks2";
    pub const CAPABILITIES: &str = "capabilities";
    pub const CLOUD_HISTORICAL_VERSIONS: &str = "cloud/historical_versions";
    pub const CLOUD_REFERENCES: &str = "cloud/references";
    pub const CLOUD_RENAME_WORKSPACE: &str = "cloud/rename_workspace";
    pub const CLOUD_ROLLBACK_WORKSPACE: &str = "cloud/rollback_workspace";
    pub const CLOUD_SHARE_WORKSPACE: &str = "cloud/share_workspace";
    pub const CLOUD_SMARTLOG_BY_VERSION: &str = "cloud/smartlog_by_version";
    pub const CLOUD_SMARTLOG: &str = "cloud/smartlog";
    pub const CLOUD_UPDATE_ARCHIVE: &str = "cloud/update_archive";
    pub const CLOUD_UPDATE_REFERENCES: &str = "cloud/update_references";
    pub const CLOUD_WORKSPACE: &str = "cloud/workspace";
    pub const CLOUD_WORKSPACES: &str = "cloud/workspaces";
    pub const COMMIT_GRAPH_SEGMENTS: &str = "commit/graph_segments";
    pub const COMMIT_GRAPH_V2: &str = "commit/graph_v2";
    pub const COMMIT_HASH_LOOKUP: &str = "commit/hash_lookup";
    pub const COMMIT_HASH_TO_LOCATION: &str = "commit/hash_to_location";
    pub const COMMIT_LOCATION_TO_HASH: &str = "commit/location_to_hash";
    pub const COMMIT_MUTATIONS: &str = "commit/mutations";
    pub const COMMIT_REVLOG_DATA: &str = "commit/revlog_data";
    pub const COMMIT_TRANSLATE_ID: &str = "commit/translate_id";
    pub const DOWNLOAD_FILE: &str = "download/file";
    pub const EPHEMERAL_PREPARE: &str = "ephemeral/prepare";
    pub const FETCH_SNAPSHOT: &str = "snapshot";
    pub const FILES2: &str = "files2";
    pub const HEALTH_CHECK: &str = "health_check";
    pub const HISTORY: &str = "history";
    pub const PATH_HISTORY: &str = "path_history";
    pub const LAND_STACK: &str = "land";
    pub const LOOKUP: &str = "lookup";
    pub const SET_BOOKMARK: &str = "bookmarks/set";
    pub const SUFFIXQUERY: &str = "suffix_query";
    pub const TREES: &str = "trees";
    pub const UPLOAD_BONSAI_CHANGESET: &str = "upload/changeset/bonsai";
    pub const UPLOAD_CHANGESETS: &str = "upload/changesets";
    pub const UPLOAD_FILENODES: &str = "upload/filenodes";
    pub const UPLOAD_TREES: &str = "upload/trees";
    pub const UPLOAD_IDENTICAL_CHANGESET: &str = "upload/changesets/identical";
    pub const UPLOAD_FILE: &str = "upload/file/";
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

pub struct ClientInner {
    config: Config,
    client: HttpClient,
}

pub struct ExpiringBool {
    inner: AtomicI64,
    origin: std::time::Instant,
    timeout: Duration,
}

impl ExpiringBool {
    fn new(timeout: Duration) -> Self {
        Self {
            inner: AtomicI64::new(-1),
            origin: std::time::Instant::now(),
            timeout,
        }
    }

    fn set(&self) {
        self.inner
            .store(self.origin.elapsed().as_secs() as i64, Ordering::Relaxed);
    }

    pub fn get(&self) -> bool {
        let val = self.inner.load(Ordering::Relaxed);
        if val >= 0 {
            let val = Duration::from_secs(val as u64);
            let now = self.origin.elapsed();
            (now - val) <= self.timeout
        } else {
            false
        }
    }
}

static LOG_SERVER_INFO_ONCE: Once = Once::new();

impl Client {
    /// Create a SaplingRemoteAPI client with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        let client = http_client("edenapi", config.http_config.clone());
        let inner = Arc::new(ClientInner { config, client });
        Self { inner }
    }

    pub(crate) fn config(&self) -> &Config {
        &self.inner.config
    }

    fn repo_name(&self) -> &str {
        &self.config().repo_name
    }

    /// Append endpoint path onto the server's base URL.
    fn build_url_repoless(&self, path: &str) -> Result<Url, SaplingRemoteApiError> {
        let url = &self.config().server_url;
        Ok(url.join(path)?)
    }

    /// Append a repo name and endpoint path onto the server's base URL.
    fn build_url(&self, path: &str) -> Result<Url, SaplingRemoteApiError> {
        let url = &self.config().server_url;
        // Repo name must be sanitized since it can be set by the user.
        let url = url
            .join(&format!("{}/", encode_repo_name(self.repo_name())))?
            .join(path)?;
        Ok(url)
    }

    /// Build URL, POST, and deserialize a single response item.
    async fn request_single<I, O>(&self, path: &str, data: I) -> Result<O, SaplingRemoteApiError>
    where
        I: ToWire,
        <O as ToWire>::Wire: Send + DeserializeOwned + 'static,
        O: ToWire + Send + 'static,
    {
        let url = self.build_url(path)?;
        let request = self
            .configure_request(path, self.inner.client.post(url))?
            .cbor(&data.to_wire())
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;
        self.fetch_single(request).await
    }

    /// Add configured values to a request.
    fn configure_request(
        &self,
        base_path: &str,
        mut req: Request,
    ) -> Result<Request, SaplingRemoteApiError> {
        // This method should probably not exist. Request configuration should flow
        // through a shared config (i.e. http_client::Config) that is applied by the
        // HttpClient. This way, every use of HttpClient does not need its own http config
        // and glue code to apply the config to the request.

        let config = self.config();

        for (k, v) in &config.headers {
            req.set_header(k, v);
        }

        let base_path = base_path.trim_matches('/');

        // Prefer per-handler timeout, falling back to generic timeout.
        if let Some(timeout) = config.handler_timeouts.get(base_path) {
            tracing::trace!(?timeout, path = base_path, "using per-handler timeout");
            req.set_timeout(*timeout);
        } else if let Some(timeout) = config.timeout {
            tracing::trace!(?timeout, path = base_path, "using generic timeout");
            req.set_timeout(timeout);
        }

        if let Some(timeout) = config.connect_timeout {
            req.set_connect_timeout(timeout);
        }

        if let Some(http_version) = config.http_version {
            req.set_http_version(http_version);
        }

        if let Some(encoding) = &config.encoding {
            req.set_accept_encoding([encoding.clone()]);
        }

        req.set_follow_redirects(config.http_config.follow_redirects);

        if let Some(mts) = config.handler_min_transfer_speeds.get(base_path) {
            tracing::trace!(
                ?mts,
                path = base_path,
                "using per-handler min transfer speed"
            );
            req.set_min_transfer_speed(*mts);
        } else if let Some(mts) = &config.min_transfer_speed {
            tracing::trace!(?mts, path = base_path, "using generic min transfer speed");
            req.set_min_transfer_speed(*mts);
        }

        Ok(req)
    }

    /// Prepare a collection of POST requests for the given keys.
    /// The keys will be grouped into batches of the specified size and
    /// passed to the `make_req` callback, which should insert them into
    /// a struct that will be CBOR-encoded and used as the request body.
    fn prepare_requests<T, K, F, R, G>(
        &self,
        fctx: Option<FetchContext>,
        base_path: &str,
        keys: K,
        batch_size: Option<usize>,
        min_batch_size: Option<usize>,
        mut make_req: F,
        mut mutate_url: G,
    ) -> Result<Vec<Request>, SaplingRemoteApiError>
    where
        K: IntoIterator<Item = T>,
        F: FnMut(Vec<T>) -> R,
        G: FnMut(&Url, &Vec<T>) -> Url,
        R: ToWire,
    {
        let url = self.build_url(base_path)?;
        split_into_batches(keys, batch_size, min_batch_size)
            .into_iter()
            .map(|keys| {
                let url = mutate_url(&url, &keys);
                let req = make_req(keys).to_wire();
                self.configure_request(base_path, self.inner.client.post(url))?
                    .cbor(&req)
                    .map_err(SaplingRemoteApiError::RequestSerializationFailed)
                    .map(|mut req| {
                        req.set_fetch_cause(fctx.as_ref().map(|fctx| fctx.cause().to_str()));
                        req
                    })
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
    ) -> Result<Response<T>, SaplingRemoteApiError> {
        let (responses, stats) = self.inner.client.send_async(requests)?;

        // Transform each response `Future` (which resolves when all of the HTTP
        // headers for that response have been received) into a `Stream` that
        // waits until all headers have been received and then starts yielding
        // entries. This allows multiplexing the streams using `select_all`.
        let streams = responses.into_iter().map(|fut| {
            stream::once(async move {
                let res = raise_for_status(fut.await?).await?;
                tracing::debug!("{:?}", ResponseMeta::from(&res));

                let res_meta = ResponseMeta::from(&res);
                let is_dogfooding = res_meta.tw_task_handle.is_some_and(|handle| { handle.contains("dogfooding") });
                if is_dogfooding {
                    RECENT_DOGFOODING_REQUESTS.set();
                }

                LOG_SERVER_INFO_ONCE.call_once(|| {
                    tracing::info!(target: "mononoke_info", mononoke_host=res_meta.mononoke_host.unwrap_or_default(), dogfooding=is_dogfooding);
                });

                Ok::<_, SaplingRemoteApiError>(res.into_body().cbor::<T>().err_into())
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
    fn fetch<T>(&self, requests: Vec<Request>) -> Result<Response<T>, SaplingRemoteApiError>
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
    ) -> Result<Response<T>, SaplingRemoteApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        guards.push(REQUESTS_INFLIGHT.entrance_guard(requests.len()));
        let Response { entries, stats } = self.fetch_raw::<<T as ToWire>::Wire>(requests)?;

        let stats = metrics::wrap_future_keep_guards(stats, guards).boxed();
        let entries = entries
            .and_then(|v| {
                future::ready(
                    v.to_api()
                        .map_err(|e| SaplingRemoteApiError::from(e.into())),
                )
            })
            .boxed();

        Ok(Response { entries, stats })
    }

    /// Similar to `fetch`. But returns a `Vec` directly.
    async fn fetch_vec<T>(&self, requests: Vec<Request>) -> Result<Vec<T>, SaplingRemoteApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.fetch::<T>(requests)?.flatten().await
    }

    /// Similar to `fetch_vec`. But with retries.
    async fn fetch_vec_with_retry<T>(
        &self,
        requests: Vec<Request>,
    ) -> Result<Vec<T>, SaplingRemoteApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.with_retry(|this| this.fetch_vec::<T>(requests.clone()).boxed())
            .await
    }

    /// Similar to `fetch_vec`. But with retries and a custom progress bar (position drops on a retry).
    async fn fetch_vec_with_retry_and_prog<T>(
        &self,
        requests: Vec<Request>,
        prog: Arc<ProgressBar>,
    ) -> Result<Vec<T>, SaplingRemoteApiError>
    where
        <T as ToWire>::Wire: Send + DeserializeOwned + 'static,
        T: ToWire + Send + 'static,
    {
        self.with_retry(|this| {
            Box::pin(async {
                let prog = prog.clone();
                prog.set_position(0);
                let resp = this
                    .fetch_guard::<T>(requests.clone(), vec![])?
                    .then(move |v| {
                        prog.increase_position(1);
                        future::ready(v)
                    });
                resp.flatten().await
            })
        })
        .await
    }

    /// Similar to `fetch`, but returns the response type directly, instead of Response<_>.
    async fn fetch_single<T>(&self, request: Request) -> Result<T, SaplingRemoteApiError>
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
        let path = log_dir.join(name);

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

    pub(crate) async fn fetch_trees(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, SaplingRemoteApiServerError>>, SaplingRemoteApiError>
    {
        tracing::info!("Fetching {} tree(s)", keys.len());

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let mut attrs = attributes.clone().unwrap_or_default();
        // Inject augmented trees attribute if configured.
        attrs = TreeAttributes {
            manifest_blob: attrs.manifest_blob,
            parents: attrs.parents,
            child_metadata: attrs.child_metadata,
            augmented_trees: attrs.augmented_trees
                || (self.config().augmented_trees && attrs.child_metadata),
        };

        let try_route_consistently = self.config().try_route_consistently;
        let min_batch_size: Option<usize> = self.config().min_batch_size;

        let requests = self.prepare_requests(
            Some(fctx),
            paths::TREES,
            keys,
            self.config().max_trees_per_batch,
            min_batch_size,
            |keys| {
                let req = TreeRequest {
                    keys,
                    attributes: attrs,
                };
                self.log_request(&req, "trees");
                req
            },
            |url, keys| {
                let mut url = url.clone();
                if try_route_consistently && keys.len() == 1 {
                    url.set_query(Some(&format!("routing_key={}", keys.first().unwrap().hgid)));
                }
                url
            },
        )?;

        self.fetch::<Result<TreeEntry, SaplingRemoteApiServerError>>(requests)
    }

    pub(crate) async fn fetch_files_attrs(
        &self,
        fctx: FetchContext,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, SaplingRemoteApiError> {
        tracing::info!("Fetching content and attributes for {} file(s)", reqs.len());

        if reqs.is_empty() {
            return Ok(Response::empty());
        }

        let guards = vec![FILES_ATTRS_INFLIGHT.entrance_guard(reqs.len())];

        let try_route_consistently = self.config().try_route_consistently;
        let min_batch_size: Option<usize> = self.config().min_batch_size;

        let requests = self.prepare_requests(
            Some(fctx),
            paths::FILES2,
            reqs,
            self.config().max_files_per_batch,
            min_batch_size,
            |reqs| {
                let req = FileRequest { reqs };
                self.log_request(&req, "files");
                req
            },
            |url, keys| {
                let mut url = url.clone();
                if try_route_consistently && keys.len() == 1 {
                    url.set_query(Some(&format!(
                        "routing_key={}",
                        keys.first().unwrap().key.hgid
                    )));
                }
                url
            },
        )?;

        self.fetch_guard::<FileResponse>(requests, guards)
    }

    /// Upload a single file
    async fn process_single_file_upload(
        &self,
        item: AnyFileContentId,
        raw_content: Bytes,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<UploadToken, SaplingRemoteApiError> {
        let mut url = self.build_url(paths::UPLOAD_FILE)?;
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
            AnyFileContentId::SeededBlake3(id) => {
                url = url.join("seeded_blake3/")?.join(&format!("{}", id))?;
            }
        }

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("content_size", &raw_content.len().to_string());
            if let Some(bubble_id) = bubble_id {
                query.append_pair("bubble_id", &bubble_id.to_string());
            }
        }

        tracing::info!("Requesting upload for {url}");

        self.fetch_single::<UploadToken>({
            self.configure_request(paths::UPLOAD_FILE, self.inner.client.put(url.clone()))?
                .body(raw_content.to_vec())
        })
        .await
    }

    // the request isn't batched, batching should be done outside if needed
    async fn upload_changesets_attempt(
        &self,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
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

        // Currently, server sends the "upload_changesets" response once it is fully completed,
        // disable min speed transfer check to avoid premature termination of requests.
        let request = self
            .configure_request(paths::UPLOAD_CHANGESETS, self.inner.client.post(url))?
            .min_transfer_speed(None)
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch::<UploadTokensResponse>(vec![request])
    }

    // the request isn't batched, batching should be done outside if needed
    async fn upload_identical_changesets_attempt(
        &self,
        changesets: Vec<IdenticalChangesetContent>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting identical changesets upload for {} item(s)",
            changesets.len(),
        );

        if changesets.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_IDENTICAL_CHANGESET)?;
        let req = UploadIdenticalChangesetsRequest { changesets }.to_wire();

        // Currently, server sends the "upload_changesets" response once it is fully completed,
        // disable min speed transfer check to avoid premature termination of requests.
        let request = self
            .configure_request(
                paths::UPLOAD_IDENTICAL_CHANGESET,
                self.inner.client.post(url),
            )?
            .min_transfer_speed(None)
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch::<UploadTokensResponse>(vec![request])
    }

    async fn commit_revlog_data_attempt(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitRevlogData>, SaplingRemoteApiError> {
        tracing::info!("Requesting revlog data for {} commit(s)", hgids.len());

        let url = self.build_url(paths::COMMIT_REVLOG_DATA)?;

        let requests = split_into_batches(hgids, self.config().max_commit_data_per_batch, None)
            .into_iter()
            .map(|hgids| {
                let req = CommitRevlogDataRequest { hgids };
                self.log_request(&req, "commit_data");
                self.configure_request(
                    paths::COMMIT_REVLOG_DATA,
                    self.inner.client.post(url.clone()),
                )?
                .cbor(&req)
                .map_err(SaplingRemoteApiError::RequestSerializationFailed)
            })
            .collect::<Result<_, _>>()?;

        self.fetch_raw::<CommitRevlogData>(requests)
    }

    async fn upload_bonsai_changeset_attempt(
        &self,
        changeset: BonsaiChangesetContent,
        bubble_id: Option<std::num::NonZeroU64>,
    ) -> Result<UploadTokensResponse, SaplingRemoteApiError> {
        tracing::info!("Requesting changeset upload");

        let mut url = self.build_url(paths::UPLOAD_BONSAI_CHANGESET)?;
        if let Some(bubble_id) = bubble_id {
            url.query_pairs_mut()
                .append_pair("bubble_id", &bubble_id.to_string());
        }
        let req = UploadBonsaiChangesetRequest { changeset }.to_wire();

        let request = self
            .configure_request(
                paths::UPLOAD_BONSAI_CHANGESET,
                self.inner.client.post(url.clone()),
            )?
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_single::<UploadTokensResponse>(request).await
    }

    async fn ephemeral_prepare_attempt(
        &self,
        custom_duration: Option<Duration>,
        labels: Option<Vec<String>>,
    ) -> Result<EphemeralPrepareResponse, SaplingRemoteApiError> {
        tracing::info!("Preparing ephemeral bubble");
        let url = self.build_url(paths::EPHEMERAL_PREPARE)?;
        let req = EphemeralPrepareRequest {
            custom_duration_secs: custom_duration.map(|d| d.as_secs()),
            labels,
        }
        .to_wire();
        let request = self
            .configure_request(paths::EPHEMERAL_PREPARE, self.inner.client.post(url))?
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        let resp = self.fetch_single::<EphemeralPrepareResponse>(request).await;
        if let Ok(ref r) = resp {
            tracing::info!("Created bubble {}", r.bubble_id);
        }
        resp
    }

    async fn fetch_snapshot_attempt(
        &self,
        request: FetchSnapshotRequest,
    ) -> Result<FetchSnapshotResponse, SaplingRemoteApiError> {
        tracing::info!("Fetching snapshot {}", request.cs_id,);
        let url = self.build_url(paths::FETCH_SNAPSHOT)?;
        let req = request.to_wire();
        let request = self
            .configure_request(paths::FETCH_SNAPSHOT, self.inner.client.post(url))?
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_single::<FetchSnapshotResponse>(request).await
    }

    /// Alter the properties of an existing snapshot
    async fn alter_snapshot_attempt(
        &self,
        request: AlterSnapshotRequest,
    ) -> Result<AlterSnapshotResponse, SaplingRemoteApiError> {
        tracing::info!("Altering snapshot {}", request.cs_id,);
        let url = self.build_url(paths::ALTER_SNAPSHOT)?;
        let req = request.to_wire();
        let request = self
            .configure_request(paths::ALTER_SNAPSHOT, self.inner.client.post(url))?
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_single::<AlterSnapshotResponse>(request).await
    }

    async fn history_attempt(
        &self,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, SaplingRemoteApiError> {
        tracing::info!("Requesting history for {} file(s)", keys.len());

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let try_route_consistently = self.config().try_route_consistently;
        let min_batch_size: Option<usize> = self.config().min_batch_size;

        let requests = self.prepare_requests(
            None,
            paths::HISTORY,
            keys,
            self.config().max_history_per_batch,
            min_batch_size,
            |keys| {
                let req = HistoryRequest { keys, length };
                self.log_request(&req, "history");
                req
            },
            |url, keys| {
                let mut url = url.clone();
                if try_route_consistently && keys.len() == 1 {
                    url.set_query(Some(&format!("routing_key={}", keys.first().unwrap().hgid)));
                }
                url
            },
        )?;

        let Response { entries, stats } = self.fetch::<HistoryResponseChunk>(requests)?;

        // Convert received `HistoryResponseChunk`s into `HistoryEntry`s.
        let entries = entries
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten()
            .boxed();

        Ok(Response { entries, stats })
    }

    async fn path_history_attempt(
        &self,
        commit: HgId,
        paths: Vec<RepoPathBuf>,
        limit: Option<u32>,
        cursor: Vec<PathHistoryRequestPaginationCursor>,
    ) -> Result<Response<PathHistoryResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting path_history for {} file(s)", paths.len());

        if paths.is_empty() {
            return Ok(Response::empty());
        }

        let requests = self.prepare_requests(
            None,
            paths::PATH_HISTORY,
            paths,
            self.config().max_path_history_per_batch,
            self.config().min_batch_size,
            |paths| {
                let cursor_for_paths = cursor
                    .clone()
                    .into_iter()
                    .filter(|c| paths.contains(&c.path))
                    .collect();
                let req = PathHistoryRequest {
                    commit,
                    paths,
                    limit,
                    cursor: cursor_for_paths,
                };
                self.log_request(&req, "path_history");
                req
            },
            |url, _paths| url.clone(),
        )?;

        self.fetch::<PathHistoryResponse>(requests)
    }

    async fn blame_attempt(
        &self,
        files: Vec<Key>,
    ) -> Result<Response<BlameResult>, SaplingRemoteApiError> {
        tracing::info!("Blaming {} file(s)", files.len());

        if files.is_empty() {
            return Ok(Response::empty());
        }

        let requests = self.prepare_requests(
            None,
            paths::BLAME,
            files,
            Some(MAX_CONCURRENT_BLAMES_PER_REQUEST),
            None,
            |files| {
                let req = BlameRequest { files };
                self.log_request(&req, "blame");
                req
            },
            |url, _keys| url.clone(),
        )?;

        self.fetch::<BlameResult>(requests)
    }

    async fn suffix_query_attempt(
        &self,
        commit: CommitId,
        suffixes: Vec<String>,
        prefixes: Option<Vec<String>>,
    ) -> Result<Response<SuffixQueryResponse>, SaplingRemoteApiError> {
        tracing::info!(
            "Retrieving file paths matching {:?} in {}",
            suffixes,
            &self.repo_name(),
        );

        if suffixes.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::SUFFIXQUERY)?;
        let req = SuffixQueryRequest {
            commit,
            basename_suffixes: suffixes,
            prefixes,
        };

        let requests = self
            .configure_request(paths::SUFFIXQUERY, self.inner.client.post(url))?
            .cbor(&req.to_wire())
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch::<SuffixQueryResponse>(vec![requests])
    }

    async fn commit_translate_id_attempt(
        &self,
        commits: Vec<CommitId>,
        scheme: CommitIdScheme,
        from_repo: Option<String>,
        to_repo: Option<String>,
    ) -> Result<Response<CommitTranslateIdResponse>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting commit id translation for {} commits into {:?}",
            commits.len(),
            scheme
        );
        let requests = self.prepare_requests(
            None,
            paths::COMMIT_TRANSLATE_ID,
            commits,
            self.config().max_commit_translate_id_per_batch,
            None,
            |commits| {
                let req = CommitTranslateIdRequest {
                    commits,
                    scheme,
                    from_repo: from_repo.clone(),
                    to_repo: to_repo.clone(),
                };
                self.log_request(&req, "commit_translate_id");
                req
            },
            |url, _keys| url.clone(),
        )?;
        self.fetch::<CommitTranslateIdResponse>(requests)
    }

    async fn download_file_attempt(
        &self,
        token: UploadToken,
    ) -> Result<Bytes, SaplingRemoteApiError> {
        tracing::info!("Downloading file");
        let url = self.build_url(paths::DOWNLOAD_FILE)?;
        let metadata = token.data.metadata.clone();
        let req = token.to_wire();
        let request = self
            .configure_request(paths::DOWNLOAD_FILE, self.inner.client.post(url.clone()))?
            .cbor(&req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        let buf = if let Some(UploadTokenMetadata::FileContentTokenMetadata(m)) = metadata {
            Vec::with_capacity(m.content_size.try_into().unwrap_or_default())
        } else {
            Vec::new()
        };

        Ok(self
            .fetch::<RawBytes>(vec![request])?
            .entries
            .try_fold(buf, |mut buf, chunk| async move {
                buf.extend_from_slice(&chunk);
                Ok(buf)
            })
            .await?
            .into())
    }

    async fn set_bookmark_attempt(
        &self,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: HashMap<String, String>,
    ) -> Result<SetBookmarkResponse, SaplingRemoteApiError> {
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
            .configure_request(paths::SET_BOOKMARK, self.inner.client.post(url))?
            .min_transfer_speed(None)
            .cbor(&set_bookmark_req.to_wire())
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_single::<SetBookmarkResponse>(req).await
    }

    /// Land a stack of commits, rebasing them onto the specified bookmark
    /// and updating the bookmark to the top of the rebased stack
    async fn land_stack_attempt(
        &self,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> Result<LandStackResponse, SaplingRemoteApiError> {
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

        // Currently, server sends the land_stack response once it is fully completed,
        // disable min speed transfer check to avoid premature termination of requests.
        let req = self
            .configure_request(paths::LAND_STACK, self.inner.client.post(url))?
            .min_transfer_speed(None)
            .cbor(&land_stack_req.to_wire())
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_single::<LandStackResponse>(req).await
    }

    async fn upload_filenodes_batch_attempt(
        &self,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting hg filenodes upload for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let requests = self.prepare_requests(
            None,
            paths::UPLOAD_FILENODES,
            items,
            Some(MAX_CONCURRENT_UPLOAD_FILENODES_PER_REQUEST),
            None,
            |ids| Batch::<_> {
                batch: ids
                    .into_iter()
                    .map(|item| UploadHgFilenodeRequest { data: item })
                    .collect(),
            },
            |url, _keys| url.clone(),
        )?;
        self.fetch::<UploadTokensResponse>(requests)
    }

    async fn upload_trees_batch_attempt(
        &self,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting trees upload for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let requests = self.prepare_requests(
            None,
            paths::UPLOAD_TREES,
            items,
            Some(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST),
            None,
            |ids| Batch::<_> {
                batch: ids
                    .into_iter()
                    .map(|item| UploadTreeRequest { entry: item })
                    .collect(),
            },
            |url, _keys| url.clone(),
        )?;

        self.fetch::<UploadTreeResponse>(requests)
    }

    async fn with_retry<'t, T>(
        &'t self,
        func: impl Fn(&'t Self) -> BoxFuture<'t, Result<T, SaplingRemoteApiError>>,
    ) -> Result<T, SaplingRemoteApiError> {
        let retry_count = self.inner.config.max_retry_per_request;
        with_retry(retry_count, || func(self)).await
    }

    async fn cloud_workspace_attempt(
        &self,
        workspace: String,
        reponame: String,
    ) -> Result<WorkspaceDataResponse, SaplingRemoteApiError> {
        tracing::info!("Requesting workspace {} in repo {} ", workspace, reponame);
        self.request_single(
            paths::CLOUD_WORKSPACE,
            CloudWorkspaceRequest {
                workspace: workspace.to_string(),
                reponame: reponame.to_string(),
            },
        )
        .await
    }

    async fn cloud_workspaces_attempt(
        &self,
        prefix: String,
        reponame: String,
    ) -> Result<WorkspacesDataResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting workspaces with prefix {} in repo {} ",
            prefix,
            reponame
        );
        self.request_single(
            paths::CLOUD_WORKSPACES,
            CloudWorkspacesRequest {
                prefix: prefix.to_string(),
                reponame: reponame.to_string(),
            },
        )
        .await
    }

    async fn cloud_references_attempt(
        &self,
        data: GetReferencesParams,
    ) -> Result<ReferencesDataResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud references for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_REFERENCES, data).await
    }

    async fn cloud_update_references_attempt(
        &self,
        data: UpdateReferencesParams,
    ) -> Result<ReferencesDataResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting update cloud references for the workspace '{}' in the repo '{}'",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_UPDATE_REFERENCES, data)
            .await
    }

    async fn cloud_smartlog_attempt(
        &self,
        data: GetSmartlogParams,
    ) -> Result<SmartlogDataResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud smartlog for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_SMARTLOG, data).await
    }

    async fn cloud_share_workspace_attempt(
        &self,
        data: CloudShareWorkspaceRequest,
    ) -> Result<CloudShareWorkspaceResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting share workspace '{}' in the repo '{}'",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_SHARE_WORKSPACE, data)
            .await
    }

    async fn cloud_update_archive_attempt(
        &self,
        data: UpdateArchiveParams,
    ) -> Result<UpdateArchiveResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud update archive for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_UPDATE_ARCHIVE, data).await
    }

    async fn cloud_rename_workspace_attempt(
        &self,
        data: RenameWorkspaceRequest,
    ) -> Result<RenameWorkspaceResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud rename workspace for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_RENAME_WORKSPACE, data)
            .await
    }

    async fn cloud_smartlog_by_version_attempt(
        &self,
        data: GetSmartlogByVersionParams,
    ) -> Result<SmartlogDataResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud smartlog for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_SMARTLOG_BY_VERSION, data)
            .await
    }

    async fn cloud_historical_versions_attempt(
        &self,
        data: HistoricalVersionsParams,
    ) -> Result<HistoricalVersionsResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud historical versions for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_HISTORICAL_VERSIONS, data)
            .await
    }

    async fn cloud_rollback_workspace_attempt(
        &self,
        data: RollbackWorkspaceRequest,
    ) -> Result<RollbackWorkspaceResponse, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting cloud rollback workspace for the workspace '{}' in the repo '{}' ",
            data.workspace,
            data.reponame
        );
        self.request_single(paths::CLOUD_ROLLBACK_WORKSPACE, data)
            .await
    }
}

#[async_trait]
impl SaplingRemoteApi for Client {
    fn url(&self) -> Option<String> {
        Some(self.config().server_url.to_string())
    }

    async fn health(&self) -> Result<ResponseMeta, SaplingRemoteApiError> {
        self.with_retry(|client| {
            async {
                let url = client.build_url_repoless(paths::HEALTH_CHECK)?;

                tracing::info!("Sending health check request: {}", &url);

                let req =
                    client.configure_request(paths::HEALTH_CHECK, client.inner.client.get(url))?;
                let res = raise_for_status(req.send_async().await?).await?;

                Ok(ResponseMeta::from(&res))
            }
            .boxed()
        })
        .await
    }

    async fn capabilities(&self) -> Result<Vec<String>, SaplingRemoteApiError> {
        self.with_retry(|client| {
            async {
                tracing::info!("Requesting capabilities for repo {}", &client.repo_name());
                let url = client.build_url(paths::CAPABILITIES)?;
                let req =
                    client.configure_request(paths::CAPABILITIES, client.inner.client.get(url))?;
                let res = raise_for_status(req.send_async().await?).await?;
                let body: Vec<u8> = res.into_body().decoded().try_concat().await?;
                let caps = serde_json::from_slice(&body)
                    .map_err(|e| SaplingRemoteApiError::ParseResponse(e.to_string()))?;
                Ok(caps)
            }
            .boxed()
        })
        .await
    }

    async fn files_attrs(
        &self,
        fctx: FetchContext,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, SaplingRemoteApiError> {
        RetryableFileAttrs::new(fctx, reqs)
            .perform_with_retries(self.clone())
            .and_then(|r| async { Ok(r.then(ready)) })
            .await
    }

    async fn history(
        &self,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, SaplingRemoteApiError> {
        self.with_retry(|this| this.history_attempt(keys.clone(), length.clone()).boxed())
            .await
    }

    async fn path_history(
        &self,
        commit: HgId,
        paths: Vec<RepoPathBuf>,
        limit: Option<u32>,
        cursor: Vec<PathHistoryRequestPaginationCursor>,
    ) -> Result<Response<PathHistoryResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.path_history_attempt(commit.clone(), paths.clone(), limit.clone(), cursor.clone())
                .boxed()
        })
        .await
    }

    async fn trees(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, SaplingRemoteApiServerError>>, SaplingRemoteApiError>
    {
        RetryableTrees::new(fctx, keys, attributes)
            .perform_with_retries(self.clone())
            .and_then(|r| async { Ok(r.then(ready)) })
            .await
    }

    async fn commit_revlog_data(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitRevlogData>, SaplingRemoteApiError> {
        self.with_retry(|this| this.commit_revlog_data_attempt(hgids.clone()).boxed())
            .await
    }

    async fn hash_prefixes_lookup(
        &self,
        prefixes: Vec<String>,
    ) -> Result<Vec<CommitHashLookupResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting full hashes for {} prefix(es)", prefixes.len());
        let prefixes: Vec<CommitHashLookupRequest> = prefixes
            .into_iter()
            .map(make_hash_lookup_request)
            .collect::<Result<Vec<CommitHashLookupRequest>, _>>()?;
        let requests = self.prepare_requests(
            None,
            paths::COMMIT_HASH_LOOKUP,
            prefixes,
            Some(MAX_CONCURRENT_HASH_LOOKUPS_PER_REQUEST),
            None,
            |prefixes| Batch::<_> { batch: prefixes },
            |url, _keys| url.clone(),
        )?;
        self.fetch_vec_with_retry::<CommitHashLookupResponse>(requests)
            .await
    }

    async fn bookmarks(
        &self,
        bookmarks: Vec<String>,
    ) -> Result<Vec<BookmarkEntry>, SaplingRemoteApiError> {
        let request_len = bookmarks.len();
        tracing::info!("Requesting {} bookmarks", bookmarks.len());
        let url = self.build_url(paths::BOOKMARKS)?;
        let bookmark_req = BookmarkRequest { bookmarks };
        self.log_request(&bookmark_req, "bookmarks");
        let bookmarks_wire = bookmark_req.to_wire();
        let req = self
            .configure_request(paths::BOOKMARKS, self.inner.client.post(url))?
            .cbor(&bookmarks_wire)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        let response = self
            .fetch_vec_with_retry::<BookmarkEntry>(vec![req])
            .await?;
        if response.len() != request_len {
            let bookmarks = bookmarks_wire.bookmarks;
            let message = format!(
                "Requested bookmarks {:?} but only got {:?}.",
                bookmarks, &response
            );
            return Err(SaplingRemoteApiError::IncompleteResponse(message));
        }
        Ok(response)
    }

    async fn bookmarks2(
        &self,
        bookmarks: Vec<String>,
        freshness: Option<Freshness>,
    ) -> Result<Vec<BookmarkResult>, SaplingRemoteApiError> {
        let request_len = bookmarks.len();
        tracing::info!(
            "Requesting {} bookmarks through bookmarks2",
            bookmarks.len()
        );
        let url = self.build_url(paths::BOOKMARKS2)?;
        let bookmark_req = Bookmark2Request {
            bookmarks,
            freshness: freshness.unwrap_or(Freshness::MaybeStale),
        };
        self.log_request(&bookmark_req, "bookmarks2");
        let bookmarks_wire = bookmark_req.to_wire();
        let req = self
            .configure_request(paths::BOOKMARKS2, self.inner.client.post(url))?
            .cbor(&bookmarks_wire)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        let response = self
            .fetch_vec_with_retry::<BookmarkResult>(vec![req])
            .await?;
        if response.len() != request_len {
            let bookmarks = bookmarks_wire.bookmarks;
            let message = format!(
                "Requested bookmarks {:?} but only got {:?}.",
                bookmarks, &response
            );
            return Err(SaplingRemoteApiError::IncompleteResponse(message));
        }
        Ok(response)
    }

    async fn set_bookmark(
        &self,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: HashMap<String, String>,
    ) -> Result<SetBookmarkResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.set_bookmark_attempt(bookmark.clone(), to, from, pushvars.clone())
                .boxed()
        })
        .await
    }

    async fn land_stack(
        &self,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> Result<LandStackResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.land_stack_attempt(bookmark.clone(), head, base, pushvars.clone())
                .boxed()
        })
        .await
    }

    async fn commit_location_to_hash(
        &self,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> Result<Vec<CommitLocationToHashResponse>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting commit location to hash (batch size = {})",
            requests.len()
        );
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let formatted = self.prepare_requests(
            None,
            paths::COMMIT_LOCATION_TO_HASH,
            requests,
            self.config().max_location_to_hash_per_batch,
            None,
            |requests| {
                let batch = CommitLocationToHashRequestBatch { requests };
                self.log_request(&batch, "commit_location_to_hash");
                batch
            },
            |url, _keys| url.clone(),
        )?;

        self.fetch_vec_with_retry::<CommitLocationToHashResponse>(formatted)
            .await
    }

    async fn commit_hash_to_location(
        &self,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitHashToLocationResponse>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting commit hash to location (batch size = {})",
            hgids.len()
        );

        if hgids.is_empty() {
            return Ok(Vec::new());
        }

        let formatted = self.prepare_requests(
            None,
            paths::COMMIT_HASH_TO_LOCATION,
            hgids,
            self.config().max_location_to_hash_per_batch,
            None,
            |hgids| {
                let batch = CommitHashToLocationRequestBatch {
                    master_heads: master_heads.clone(),
                    hgids,
                    unfiltered: Some(true),
                };
                self.log_request(&batch, "commit_hash_to_location");
                batch
            },
            |url, _keys| url.clone(),
        )?;

        self.fetch_vec_with_retry::<CommitHashToLocationResponse>(formatted)
            .await
    }

    async fn commit_known(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitKnownResponse>, SaplingRemoteApiError> {
        let anyids: Vec<_> = hgids.iter().cloned().map(AnyId::HgChangesetId).collect();
        let entries = self.lookup_batch(anyids.clone(), None, None).await?;

        let into_hgid = |id: IndexableId| match id.id {
            AnyId::HgChangesetId(hgid) => Ok(hgid),
            _ => Err(SaplingRemoteApiError::Other(format_err!(
                "Invalid id returned"
            ))),
        };

        let id_to_token: HashMap<HgId, Option<UploadToken>> = entries
            .into_iter()
            .map(|response| match response.result {
                LookupResult::NotPresent(id) => Ok((into_hgid(id)?, None)),
                LookupResult::Present(token) => Ok((into_hgid(token.indexable_id())?, Some(token))),
            })
            .collect::<Result<_, SaplingRemoteApiError>>()?;

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
    ) -> Result<Vec<CommitGraphEntry>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting commit graph with {} heads and {} common",
            heads.len(),
            common.len(),
        );
        let url = self.build_url(paths::COMMIT_GRAPH_V2)?;
        let graph_req = CommitGraphRequest { heads, common };
        self.log_request(&graph_req, "commit_graph");
        let wire_graph_req = graph_req.to_wire();

        // In the current implementation, server may send all CommitGraph nodes
        // at once on completion, or may send graph nodes gradually (streaming).
        // Since, it depends on request, min speed transfer check must be disabled.
        // Since we have a special progress bar and response is small, let's disable compression of
        // response's body.
        let req = self
            .configure_request(paths::COMMIT_GRAPH_V2, self.inner.client.post(url))?
            .accept_encoding([Encoding::Identity])
            .min_transfer_speed(None)
            .cbor(&wire_graph_req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        let prog = ProgressBar::new_detached("commit graph", 0, "commits fetched");
        self.fetch_vec_with_retry_and_prog::<CommitGraphEntry>(vec![req], prog)
            .await
    }

    async fn commit_graph_segments(
        &self,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Vec<CommitGraphSegmentsEntry>, SaplingRemoteApiError> {
        tracing::info!(
            "Requesting commit graph segments with {} heads and {} common",
            heads.len(),
            common.len(),
        );
        let url = self.build_url(paths::COMMIT_GRAPH_SEGMENTS)?;
        let graph_req = CommitGraphSegmentsRequest { heads, common };
        self.log_request(&graph_req, "commit_graph_segments");
        let wire_graph_req = graph_req.to_wire();

        let req = self
            .configure_request(paths::COMMIT_GRAPH_SEGMENTS, self.inner.client.post(url))?
            .min_transfer_speed(None)
            .cbor(&wire_graph_req)
            .map_err(SaplingRemoteApiError::RequestSerializationFailed)?;

        self.fetch_vec_with_retry::<CommitGraphSegmentsEntry>(vec![req])
            .await
    }

    async fn lookup_batch(
        &self,
        items: Vec<AnyId>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Vec<LookupResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting lookup for {} item(s)", items.len());

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let requests = self.prepare_requests(
            None,
            paths::LOOKUP,
            items,
            Some(MAX_CONCURRENT_LOOKUPS_PER_REQUEST),
            None,
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
            |url, _keys| url.clone(),
        )?;

        self.fetch_vec_with_retry::<LookupResponse>(requests).await
    }

    async fn process_files_upload(
        &self,
        data: Vec<(AnyFileContentId, Bytes)>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
        lookup_policy: UploadLookupPolicy,
    ) -> Result<Response<UploadToken>, SaplingRemoteApiError> {
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

        if lookup_policy == UploadLookupPolicy::PerformLookup {
            let entries = self
                .lookup_batch(anyids.clone(), bubble_id, copy_from_bubble_id)
                .await?;
            for entry in entries {
                if let LookupResult::Present(token) = entry.result {
                    uploaded_ids.insert(token.indexable_id());
                    uploaded_tokens.push(token);
                }
            }

            tracing::info!(
                "Received {} token(s) from the lookup_batch request",
                uploaded_tokens.len()
            );
        }

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
                    self.with_retry(|this| {
                        this.process_single_file_upload(id, content.clone(), bubble_id)
                            .boxed()
                    })
                    .await
                }),
        )
        .buffer_unordered(MAX_CONCURRENT_FILE_UPLOADS)
        .collect::<Vec<_>>()
        .await;

        tracing::info!(
            "Received {} new token(s) from upload requests",
            new_tokens.iter().filter(|x| x.is_ok()).count()
        );

        // Merge all the tokens together
        let all_tokens = new_tokens
            .into_iter()
            .chain(uploaded_tokens.into_iter().map(Ok))
            .collect::<Vec<Result<_, _>>>();

        Ok(Response {
            stats: Box::pin(async { Ok(Default::default()) }),
            entries: Box::pin(futures::stream::iter(all_tokens)),
        })
    }

    async fn upload_filenodes_batch(
        &self,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| this.upload_filenodes_batch_attempt(items.clone()).boxed())
            .await
    }

    async fn upload_trees_batch(
        &self,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| this.upload_trees_batch_attempt(items.clone()).boxed())
            .await
    }

    async fn upload_changesets(
        &self,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.upload_changesets_attempt(changesets.clone(), mutations.clone())
                .boxed()
        })
        .await
    }

    async fn upload_identical_changesets(
        &self,
        changesets: Vec<IdenticalChangesetContent>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.upload_identical_changesets_attempt(changesets.clone())
                .boxed()
        })
        .await
    }

    async fn upload_bonsai_changeset(
        &self,
        changeset: BonsaiChangesetContent,
        bubble_id: Option<std::num::NonZeroU64>,
    ) -> Result<UploadTokensResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.upload_bonsai_changeset_attempt(changeset.clone(), bubble_id)
                .boxed()
        })
        .await
    }

    async fn ephemeral_prepare(
        &self,
        custom_duration: Option<Duration>,
        labels: Option<Vec<String>>,
    ) -> Result<EphemeralPrepareResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.ephemeral_prepare_attempt(custom_duration, labels.clone())
                .boxed()
        })
        .await
    }

    async fn fetch_snapshot(
        &self,
        request: FetchSnapshotRequest,
    ) -> Result<FetchSnapshotResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.fetch_snapshot_attempt(request.clone()).boxed())
            .await
    }

    /// Alter the properties of an existing snapshot
    async fn alter_snapshot(
        &self,
        request: AlterSnapshotRequest,
    ) -> Result<AlterSnapshotResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.alter_snapshot_attempt(request.clone()).boxed())
            .await
    }

    async fn download_file(&self, token: UploadToken) -> Result<Bytes, SaplingRemoteApiError> {
        self.with_retry(|this| this.download_file_attempt(token.clone()).boxed())
            .await
    }

    async fn commit_mutations(
        &self,
        commits: Vec<HgId>,
    ) -> Result<Vec<CommitMutationsResponse>, SaplingRemoteApiError> {
        tracing::info!("Requesting mutation info for {} commit(s)", commits.len());
        let requests = self.prepare_requests(
            None,
            paths::COMMIT_MUTATIONS,
            commits,
            self.config().max_commit_mutations_per_batch,
            None,
            |commits| {
                let req = CommitMutationsRequest { commits };
                self.log_request(&req, "commit_mutations");
                req
            },
            |url, _keys| url.clone(),
        )?;

        self.fetch_vec_with_retry::<CommitMutationsResponse>(requests)
            .await
    }

    async fn commit_translate_id(
        &self,
        commits: Vec<CommitId>,
        scheme: CommitIdScheme,
        from_repo: Option<String>,
        to_repo: Option<String>,
    ) -> Result<Response<CommitTranslateIdResponse>, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.commit_translate_id_attempt(
                commits.clone(),
                scheme.clone(),
                from_repo.clone(),
                to_repo.clone(),
            )
            .boxed()
        })
        .await
    }

    async fn blame(&self, files: Vec<Key>) -> Result<Response<BlameResult>, SaplingRemoteApiError> {
        self.with_retry(|this| this.blame_attempt(files.clone()).boxed())
            .await
    }

    async fn cloud_workspace(
        &self,
        workspace: String,
        reponame: String,
    ) -> Result<WorkspaceDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.cloud_workspace_attempt(workspace.clone(), reponame.clone())
                .boxed()
        })
        .await
    }

    async fn cloud_workspaces(
        &self,
        prefix: String,
        reponame: String,
    ) -> Result<WorkspacesDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| {
            this.cloud_workspaces_attempt(prefix.clone(), reponame.clone())
                .boxed()
        })
        .await
    }

    async fn cloud_references(
        &self,
        data: GetReferencesParams,
    ) -> Result<ReferencesDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_references_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_update_references(
        &self,
        data: UpdateReferencesParams,
    ) -> Result<ReferencesDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_update_references_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_smartlog(
        &self,
        data: GetSmartlogParams,
    ) -> Result<SmartlogDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_smartlog_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_share_workspace(
        &self,
        data: CloudShareWorkspaceRequest,
    ) -> Result<CloudShareWorkspaceResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_share_workspace_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_update_archive(
        &self,
        data: UpdateArchiveParams,
    ) -> Result<UpdateArchiveResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_update_archive_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_rename_workspace(
        &self,
        data: RenameWorkspaceRequest,
    ) -> Result<RenameWorkspaceResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_rename_workspace_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_smartlog_by_version(
        &self,
        data: GetSmartlogByVersionParams,
    ) -> Result<SmartlogDataResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_smartlog_by_version_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_historical_versions(
        &self,
        data: HistoricalVersionsParams,
    ) -> Result<HistoricalVersionsResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_historical_versions_attempt(data.clone()).boxed())
            .await
    }

    async fn cloud_rollback_workspace(
        &self,
        data: RollbackWorkspaceRequest,
    ) -> Result<RollbackWorkspaceResponse, SaplingRemoteApiError> {
        self.with_retry(|this| this.cloud_rollback_workspace_attempt(data.clone()).boxed())
            .await
    }

    async fn suffix_query(
        &self,
        commit: CommitId,
        suffixes: Vec<String>,
        prefixes: Option<Vec<String>>,
    ) -> Result<Response<SuffixQueryResponse>, SaplingRemoteApiError> {
        // Clone required here due to closure possibly being run more than once
        self.with_retry(|this| {
            this.suffix_query_attempt(commit.clone(), suffixes.clone(), prefixes.clone())
                .boxed()
        })
        .await
    }
}

/// Split up a collection of keys into batches of at most `batch_size`.
fn split_into_batches<T>(
    keys: impl IntoIterator<Item = T>,
    batch_size: Option<usize>,
    min_batch_size: Option<usize>,
) -> Vec<Vec<T>> {
    match batch_size {
        Some(n) => {
            let mut chunks_vec = Vec::new();
            for chunk in keys.into_iter().chunks(n).into_iter() {
                let v = Vec::from_iter(chunk);
                // This bit is used to not construct small batches
                // because they are not routed consistently and
                // because of that are subuptimal.
                if let Some(min_batch_size) = min_batch_size {
                    if v.len() >= min_batch_size {
                        chunks_vec.push(v);
                    } else {
                        for key in v.into_iter() {
                            chunks_vec.push(vec![key]);
                        }
                    }
                } else {
                    chunks_vec.push(v);
                }
            }

            chunks_vec
        }
        None => vec![keys.into_iter().collect()],
    }
}

async fn raise_for_status(res: AsyncResponse) -> Result<AsyncResponse, SaplingRemoteApiError> {
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
    Err(SaplingRemoteApiError::HttpError {
        status,
        message,
        headers,
        url,
    })
}

async fn with_retry<'t, T>(
    max_retry_count: usize,
    func: impl Fn() -> BoxFuture<'t, Result<T, SaplingRemoteApiError>>,
) -> Result<T, SaplingRemoteApiError> {
    let mut attempt = 0usize;
    loop {
        let result = func().await;
        if attempt >= max_retry_count {
            return result;
        }
        match result {
            Ok(result) => return Ok(result),
            Err(ref error) => match error.retry_after(attempt, max_retry_count) {
                Some(sleep_time) => {
                    tracing::warn!("Retrying http error {:?}", error);
                    tokio::time::sleep(sleep_time).await;
                }
                None => {
                    return result;
                }
            },
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use anyhow::Result;

    use crate::builder::HttpClientBuilder;
    use crate::client::split_into_batches;

    #[test]
    fn test_split_into_batches() -> Result<()> {
        let keys = vec![1, 2, 3];
        let result = split_into_batches(keys, Some(2), None);
        assert_eq!(vec![vec![1, 2], vec![3]], result);

        let keys = vec![1, 2, 3, 4];
        let result = split_into_batches(keys, Some(2), None);
        assert_eq!(vec![vec![1, 2], vec![3, 4]], result);

        let keys = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let result = split_into_batches(keys, Some(4), Some(3));
        assert_eq!(
            vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9], vec![10]],
            result
        );

        let keys = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let result = split_into_batches(keys, Some(4), None);
        assert_eq!(
            vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10]],
            result
        );

        Ok(())
    }

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

    #[test]
    fn test_expiring_bool() {
        let expiring = crate::client::ExpiringBool::new(Duration::from_secs(1));
        assert!(!expiring.get());
        expiring.set();
        assert!(expiring.get());
        thread::sleep(Duration::from_secs(1));
        assert!(!expiring.get());
    }
}
