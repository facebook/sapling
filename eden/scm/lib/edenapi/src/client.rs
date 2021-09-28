/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes as RawBytes;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fmt::Debug;
use std::fs::{create_dir_all, File};
use std::iter::FromIterator;
use std::num::NonZeroU64;
use std::sync::Arc;

use anyhow::{format_err, Context};
use async_trait::async_trait;
use futures::prelude::*;
use itertools::Itertools;
use minibytes::Bytes;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_cbor::Deserializer;
use url::Url;

use auth::check_certs;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitKnownResponse;
use edenapi_types::{
    json::ToJson,
    make_hash_lookup_request,
    wire::{
        WireBookmarkEntry, WireCloneData, WireCommitGraphEntry, WireCommitHashLookupResponse,
        WireCommitHashToLocationResponse, WireCommitLocationToHashResponse,
        WireEphemeralPrepareResponse, WireFetchSnapshotResponse, WireFileEntry,
        WireHistoryResponseChunk, WireIdMapEntry, WireLookupResponse, WireToApiConversionError,
        WireTreeEntry, WireUploadToken, WireUploadTokensResponse, WireUploadTreeResponse,
    },
    AnyFileContentId, AnyId, Batch, BonsaiChangesetContent, BookmarkEntry, BookmarkRequest,
    CloneData, CommitGraphRequest, CommitHashLookupRequest, CommitHashLookupResponse,
    CommitHashToLocationRequestBatch, CommitHashToLocationResponse, CommitLocationToHashRequest,
    CommitLocationToHashRequestBatch, CommitLocationToHashResponse, CommitRevlogData,
    CommitRevlogDataRequest, CompleteTreeRequest, EdenApiServerError, EphemeralPrepareRequest,
    EphemeralPrepareResponse, FetchSnapshotRequest, FetchSnapshotResponse, FileEntry, FileRequest,
    FileSpec, HgFilenodeData, HgMutationEntryContent, HistoryEntry, HistoryRequest, LookupRequest,
    LookupResponse, ServerError, ToApi, ToWire, TreeAttributes, TreeEntry, TreeRequest,
    UploadBonsaiChangesetRequest, UploadHgChangeset, UploadHgChangesetsRequest,
    UploadHgFilenodeRequest, UploadToken, UploadTokenMetadata, UploadTokensResponse,
    UploadTreeEntry, UploadTreeRequest, UploadTreeResponse,
};
use hg_http::http_client;
use http_client::{AsyncResponse, HttpClient, HttpClientError, Request};
use types::{HgId, Key, RepoPathBuf};

use crate::api::EdenApi;
use crate::builder::Config;
use crate::errors::EdenApiError;
use crate::response::{Response, ResponseMeta};
use crate::types::wire::pull::PullFastForwardRequest;
use metrics::{Counter, EntranceGuard};
use std::time::Duration;

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
    pub const FILES: &str = "files";
    pub const HISTORY: &str = "history";
    pub const TREES: &str = "trees";
    pub const COMPLETE_TREES: &str = "trees/complete";
    pub const COMMIT_REVLOG_DATA: &str = "commit/revlog_data";
    pub const CLONE_DATA: &str = "clone";
    pub const PULL_FAST_FORWARD: &str = "pull_fast_forward_master";
    pub const FULL_IDMAP_CLONE_DATA: &str = "full_idmap_clone";
    pub const COMMIT_LOCATION_TO_HASH: &str = "commit/location_to_hash";
    pub const COMMIT_HASH_TO_LOCATION: &str = "commit/hash_to_location";
    pub const COMMIT_HASH_LOOKUP: &str = "commit/hash_lookup";
    pub const COMMIT_GRAPH: &str = "commit/graph";
    pub const BOOKMARKS: &str = "bookmarks";
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
}

impl Client {
    /// Create an EdenAPI client with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        let client = http_client("edenapi")
            .verbose(config.debug)
            .max_concurrent_requests(config.max_requests.unwrap_or(0));
        let inner = Arc::new(ClientInner { config, client });
        Self { inner }
    }

    fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Append a repo name and endpoint path onto the server's base URL.
    fn build_url(&self, path: &str, repo: Option<&str>) -> Result<Url, EdenApiError> {
        let url = &self.config().server_url;
        Ok(match repo {
            Some(repo) => url
                // Repo name must be sanitized since it can be set by the user.
                .join(&format!("{}/", utf8_percent_encode(repo, RESERVED_CHARS)))?
                .join(path)?,
            None => url.join(path)?,
        })
    }

    /// Add configured values to a request.
    fn configure_request(&self, mut req: Request) -> Result<Request, EdenApiError> {
        let config = self.config();

        if let Some(ref cert) = config.cert {
            if self.config().validate_certs {
                check_certs(cert)?;
            }
            req.set_cert(cert);
            req.set_convert_cert(config.convert_cert);
        }

        if let Some(ref key) = config.key {
            req.set_key(key);
        }

        if let Some(ref ca) = config.ca_bundle {
            req.set_cainfo(ca);
        }

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
        R: Serialize,
    {
        split_into_batches(keys, batch_size)
            .into_iter()
            .map(|keys| {
                let req = make_req(keys);
                self.configure_request(Request::post(url.clone()))?
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
    fn fetch<T>(&self, requests: Vec<Request>) -> Result<Response<<T as ToApi>::Api>, EdenApiError>
    where
        T: ToApi + Send + DeserializeOwned + 'static,
        <T as ToApi>::Api: Send + 'static,
    {
        self.fetch_guard::<T>(requests, vec![])
    }

    fn fetch_guard<T>(
        &self,
        requests: Vec<Request>,
        mut guards: Vec<EntranceGuard>,
    ) -> Result<Response<<T as ToApi>::Api>, EdenApiError>
    where
        T: ToApi + Send + DeserializeOwned + 'static,
        <T as ToApi>::Api: Send + 'static,
    {
        guards.push(REQUESTS_INFLIGHT.entrance_guard(requests.len()));
        let Response { entries, stats } = self.fetch_raw::<T>(requests)?;

        let stats = metrics::wrap_future_keep_guards(stats, guards).boxed();
        let entries = entries
            .and_then(|v| future::ready(v.to_api().map_err(|e| EdenApiError::from(e.into()))))
            .boxed();

        Ok(Response { entries, stats })
    }


    /// Log the request to the configured log directory as JSON.
    fn log_request<R: ToJson + Debug>(&self, req: &R, label: &str) {
        tracing::trace!("Sending request: {:?}", req);

        let log_dir = match &self.config().log_dir {
            Some(path) => path.clone(),
            None => return,
        };

        let json = req.to_json();
        let timestamp = chrono::Local::now().format("%y%m%d_%H%M%S_%f");
        let name = format!("{}_{}.json", &timestamp, label);
        let path = log_dir.join(&name);

        let _ = async_runtime::spawn_blocking(move || {
            if let Err(e) = || -> anyhow::Result<()> {
                create_dir_all(&log_dir)?;
                let file = File::create(&path)?;

                // Log as prettified JSON so that requests are easy for humans
                // to edit when debugging issues. Should not be a problem for
                // normal usage since logging is disabled by default.
                serde_json::to_writer_pretty(file, &json)?;
                Ok(())
            }() {
                tracing::warn!("Failed to log request: {:?}", &e);
            }
        });
    }

    /// Upload a single file
    async fn process_single_file_upload(
        &self,
        repo: String,
        item: AnyFileContentId,
        raw_content: Bytes,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadToken>, EdenApiError> {
        let mut url = self.build_url(paths::UPLOAD, Some(&repo))?;
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

        Ok(self.fetch::<WireUploadToken>(vec![{
            let request = self
                .configure_request(Request::put(url.clone()))?
                .body(raw_content.to_vec());
            request
        }])?)
    }

    async fn clone_data_retry(&self, repo: String) -> Result<CloneData<HgId>, EdenApiError> {
        const CLONE_ATTEMPTS_MAX: usize = 10;
        let mut attempt = 0usize;
        loop {
            let result = self.clone_data_attempt(&repo).await;
            if attempt >= CLONE_ATTEMPTS_MAX {
                return result;
            }
            match result {
                Err(EdenApiError::HttpError { status, message }) => {
                    tracing::warn!("Retrying http status {} {}", status, message);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(EdenApiError::Http(err)) => {
                    tracing::warn!("Retrying http error {:?}", err);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                other => return other,
            }
            attempt += 1;
        }
    }

    async fn clone_data_attempt(&self, repo: &str) -> Result<CloneData<HgId>, EdenApiError> {
        let url = self.build_url(paths::CLONE_DATA, Some(repo))?;
        let req = self.configure_request(Request::post(url))?;
        let mut fetch = self.fetch::<WireCloneData>(vec![req])?;
        fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })?
    }

    async fn fast_forward_pull_retry(
        &self,
        repo: String,
        req: PullFastForwardRequest,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        const PULL_ATTEMPTS_MAX: usize = 10;
        let mut attempt = 0usize;
        loop {
            let result = self.fast_forward_pull_attempt(&repo, req.clone()).await;
            if attempt >= PULL_ATTEMPTS_MAX {
                return result;
            }
            match result {
                Err(EdenApiError::HttpError { status, message }) => {
                    tracing::warn!("Retrying http status {} {}", status, message);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(EdenApiError::Http(err)) => {
                    tracing::warn!("Retrying http error {:?}", err);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                other => return other,
            }
            attempt += 1;
        }
    }

    async fn fast_forward_pull_attempt(
        &self,
        repo: &str,
        req: PullFastForwardRequest,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let url = self.build_url(paths::PULL_FAST_FORWARD, Some(&repo))?;
        let req = self
            .configure_request(Request::post(url))?
            .cbor(&req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;
        let mut fetch = self.fetch::<WireCloneData>(vec![req])?;
        fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })?
    }
}

#[async_trait]
impl EdenApi for Client {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        let url = self.build_url(paths::HEALTH_CHECK, None)?;

        let msg = format!("Sending health check request: {}", &url);
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let req = self.configure_request(Request::get(url))?;
        let res = raise_for_status(req.send_async().await?).await?;

        Ok(ResponseMeta::from(&res))
    }

    async fn files(
        &self,
        repo: String,
        keys: Vec<Key>,
    ) -> Result<Response<FileEntry>, EdenApiError> {
        let msg = format!("Requesting content for {} file(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let guards = vec![FILES_INFLIGHT.entrance_guard(keys.len())];

        let url = self.build_url(paths::FILES, Some(&repo))?;
        let requests = self.prepare_requests(&url, keys, self.config().max_files, |keys| {
            let req = FileRequest { keys, reqs: vec![] };
            self.log_request(&req, "files");
            req.to_wire()
        })?;

        Ok(self.fetch_guard::<WireFileEntry>(requests, guards)?)
    }

    async fn files_attrs(
        &self,
        repo: String,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileEntry>, EdenApiError> {
        let msg = format!("Requesting attributes for {} file(s)", reqs.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if reqs.is_empty() {
            return Ok(Response::empty());
        }

        let guards = vec![FILES_ATTRS_INFLIGHT.entrance_guard(reqs.len())];

        let url = self.build_url(paths::FILES, Some(&repo))?;
        let requests = self.prepare_requests(&url, reqs, self.config().max_files, |reqs| {
            let req = FileRequest { reqs, keys: vec![] };
            self.log_request(&req, "files");
            req.to_wire()
        })?;

        Ok(self.fetch_guard::<WireFileEntry>(requests, guards)?)
    }

    async fn history(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, EdenApiError> {
        let msg = format!("Requesting history for {} file(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::HISTORY, Some(&repo))?;
        let requests = self.prepare_requests(&url, keys, self.config().max_history, |keys| {
            let req = HistoryRequest { keys, length };
            self.log_request(&req, "history");
            req.to_wire()
        })?;

        let Response { entries, stats } = self.fetch::<WireHistoryResponseChunk>(requests)?;

        // Convert received `HistoryResponseChunk`s into `HistoryEntry`s.
        let entries = entries
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten()
            .boxed();

        Ok(Response { entries, stats })
    }

    async fn trees(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let msg = format!("Requesting {} tree(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::TREES, Some(&repo))?;
        let requests = self.prepare_requests(&url, keys, self.config().max_trees, |keys| {
            let req = TreeRequest {
                keys,
                attributes: attributes.clone().unwrap_or_default(),
            };
            self.log_request(&req, "trees");
            req.to_wire()
        })?;

        Ok(self.fetch::<WireTreeEntry>(requests)?)
    }

    async fn complete_trees(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let msg = format!(
            "Requesting {} complete tree(s) for directory '{}'",
            mfnodes.len(),
            &rootdir
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let url = self.build_url(paths::COMPLETE_TREES, Some(&repo))?;
        let tree_req = CompleteTreeRequest {
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        };
        self.log_request(&tree_req, "complete_trees");
        let wire_tree_req = tree_req.to_wire();

        let req = self
            .configure_request(Request::post(url))?
            .cbor(&wire_tree_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireTreeEntry>(vec![req])?)
    }

    async fn commit_revlog_data(
        &self,
        repo: String,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitRevlogData>, EdenApiError> {
        let msg = format!("Requesting revlog data for {} commit(s)", hgids.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let url = self.build_url(paths::COMMIT_REVLOG_DATA, Some(&repo))?;
        let commit_revlog_data_req = CommitRevlogDataRequest { hgids };

        self.log_request(&commit_revlog_data_req, "commit_revlog_data");

        let req = self
            .configure_request(Request::post(url))?
            .cbor(&commit_revlog_data_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_raw::<CommitRevlogData>(vec![req])
    }

    async fn hash_prefixes_lookup(
        &self,
        repo: String,
        prefixes: Vec<String>,
    ) -> Result<Response<CommitHashLookupResponse>, EdenApiError> {
        let msg = format!("Requesting full hashes for {} prefix(es)", prefixes.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }
        let url = self.build_url(paths::COMMIT_HASH_LOOKUP, Some(&repo))?;
        let prefixes: Vec<CommitHashLookupRequest> = prefixes
            .into_iter()
            .map(|prefix| make_hash_lookup_request(prefix))
            .collect::<Result<Vec<CommitHashLookupRequest>, _>>()?;
        let requests = self.prepare_requests(
            &url,
            prefixes,
            Some(MAX_CONCURRENT_HASH_LOOKUPS_PER_REQUEST),
            |prefixes| {
                let req = Batch::<_> {
                    batch: prefixes.into_iter().map(|prefix| prefix).collect(),
                };
                req.to_wire()
            },
        )?;
        Ok(self.fetch::<WireCommitHashLookupResponse>(requests)?)
    }


    async fn bookmarks(
        &self,
        repo: String,
        bookmarks: Vec<String>,
    ) -> Result<Response<BookmarkEntry>, EdenApiError> {
        let msg = format!("Requesting '{}' bookmarks", bookmarks.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }
        let url = self.build_url(paths::BOOKMARKS, Some(&repo))?;
        let bookmark_req = BookmarkRequest { bookmarks };
        self.log_request(&bookmark_req, "bookmarks");
        let req = self
            .configure_request(Request::post(url))?
            .cbor(&bookmark_req.to_wire())
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireBookmarkEntry>(vec![req])?)
    }

    async fn clone_data(&self, repo: String) -> Result<CloneData<HgId>, EdenApiError> {
        let msg = format!("Requesting clone data for the '{}' repository", repo);
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        self.clone_data_retry(repo).await
    }

    async fn pull_fast_forward_master(
        &self,
        repo: String,
        old_master: HgId,
        new_master: HgId,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let msg = format!(
            "Requesting pull fast forward data for the '{}' repository",
            repo
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let req = PullFastForwardRequest {
            old_master,
            new_master,
        };
        self.fast_forward_pull_retry(repo, req).await
    }

    async fn full_idmap_clone_data(&self, repo: String) -> Result<CloneData<HgId>, EdenApiError> {
        let msg = format!(
            "Requesting full idmap clone data for the '{}' repository",
            repo
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let url = self.build_url(paths::FULL_IDMAP_CLONE_DATA, Some(&repo))?;
        let req = self.configure_request(Request::post(url))?;
        let async_response = req
            .send_async()
            .await
            .context("error receiving async response")?;
        let response_bytes = async_response
            .into_body()
            .decoded()
            .try_concat()
            .await
            .context("error receiving bytes from server")?;

        let mut deserializer = Deserializer::from_slice(&response_bytes);
        let wire_clone_data =
            WireCloneData::deserialize(&mut deserializer).map_err(HttpClientError::from)?;

        let mut clone_data = wire_clone_data.to_api()?;

        let idmap = deserializer
            .into_iter::<WireIdMapEntry>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(HttpClientError::from)?
            .into_iter()
            .map(|e| Ok((e.dag_id.to_api()?, e.hg_id.to_api()?)))
            .collect::<Result<HashMap<_, _>, WireToApiConversionError>>()?;

        clone_data.idmap = idmap;
        Ok(clone_data)
    }

    async fn commit_location_to_hash(
        &self,
        repo: String,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> Result<Response<CommitLocationToHashResponse>, EdenApiError> {
        let msg = format!(
            "Requesting commit location to hash (batch size = {})",
            requests.len()
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if requests.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::COMMIT_LOCATION_TO_HASH, Some(&repo))?;

        let formatted = self.prepare_requests(
            &url,
            requests,
            self.config().max_location_to_hash,
            |requests| {
                let batch = CommitLocationToHashRequestBatch { requests };
                self.log_request(&batch, "commit_location_to_hash");
                batch.to_wire()
            },
        )?;

        Ok(self.fetch::<WireCommitLocationToHashResponse>(formatted)?)
    }

    async fn commit_hash_to_location(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitHashToLocationResponse>, EdenApiError> {
        let msg = format!(
            "Requesting commit hash to location (batch size = {})",
            hgids.len()
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if hgids.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::COMMIT_HASH_TO_LOCATION, Some(&repo))?;

        let formatted =
            self.prepare_requests(&url, hgids, self.config().max_location_to_hash, |hgids| {
                let batch = CommitHashToLocationRequestBatch {
                    master_heads: master_heads.clone(),
                    hgids,
                    unfiltered: Some(true),
                };
                self.log_request(&batch, "commit_hash_to_location");
                batch.to_wire()
            })?;

        Ok(self.fetch::<WireCommitHashToLocationResponse>(formatted)?)
    }

    async fn commit_known(
        &self,
        repo: String,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitKnownResponse>, EdenApiError> {
        let response = self
            .lookup_batch(
                repo,
                hgids
                    .clone()
                    .into_iter()
                    .map(|hgid| AnyId::HgChangesetId(hgid))
                    .collect(),
                None,
            )
            .await?;

        let (mut entries, stats) = (response.entries, response.stats);

        let mut knowns: Vec<Option<bool>> = vec![None; hgids.len()];

        // Convert `lookup_batch` to vec<Option<bool> with token validation (check `HgChangesetId` in the token is correct for the index).
        // `Some(true)`: The server verified that `hgid` is known.
        // `Some(false)`: The server does not known `hgid`.
        // `None`: The server failed to check `hgid` due to some error.
        //         The existing API doesn't provide information for what id was the error,
        //         so log the original error and convert it to a generic "the server cannot check `HgChangesetId`"

        while let Some(entry) = entries.next().await {
            match entry {
                Ok(entry) => {
                    if entry.index >= hgids.len() {
                        return Err(EdenApiError::Other(format_err!(
                            "`lookup_batch` returned an invalid index"
                        )));
                    }
                    match entry.token {
                        Some(token) => {
                            if let AnyId::HgChangesetId(token_id) = token.data.id {
                                if token_id != hgids[entry.index] {
                                    return Err(EdenApiError::Other(format_err!(
                                        "`lookup_batch` returned an invalid token or an invalid index"
                                    )));
                                }
                                knowns[entry.index] = Some(true)
                            } else {
                                return Err(EdenApiError::Other(format_err!(
                                    "`lookup_batch` returned an invalid token"
                                )));
                            }
                        }
                        None => knowns[entry.index] = Some(false),
                    }
                }
                Err(err) => {
                    tracing::warn!("`lookup_batch` error: {:?}", &err);
                }
            }
        }

        Ok(Response {
            stats,
            entries: Box::pin(futures::stream::iter(
                knowns
                    .into_iter()
                    .enumerate()
                    .map(|(index, res)| match res {
                        Some(value) => CommitKnownResponse {
                            hgid: hgids[index],
                            known: Ok(value),
                        },
                        None => CommitKnownResponse {
                            hgid: hgids[index],
                            known: Err(ServerError::generic(
                                "the server cannot check `HgChangesetId`",
                            )),
                        },
                    })
                    .collect::<Vec<CommitKnownResponse>>()
                    .into_iter()
                    .map(Ok),
            )),
        })
    }

    async fn commit_graph(
        &self,
        repo: String,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Response<CommitGraphEntry>, EdenApiError> {
        tracing::info!(
            "Requesting commit graph with {} heads and {} common",
            heads.len(),
            common.len()
        );
        let url = self.build_url(paths::COMMIT_GRAPH, Some(&repo))?;
        let graph_req = CommitGraphRequest { heads, common };
        self.log_request(&graph_req, "commit_graph");
        let wire_graph_req = graph_req.to_wire();

        let req = self
            .configure_request(Request::post(url))?
            .cbor(&wire_graph_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch::<WireCommitGraphEntry>(vec![req])
    }

    async fn lookup_batch(
        &self,
        repo: String,
        items: Vec<AnyId>,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<LookupResponse>, EdenApiError> {
        let msg = format!("Requesting lookup for {} item(s)", items.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::LOOKUP, Some(&repo))?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_LOOKUPS_PER_REQUEST),
            |ids| {
                let req = Batch::<LookupRequest> {
                    batch: ids
                        .into_iter()
                        .map(|id| LookupRequest { id, bubble_id })
                        .collect(),
                };
                req.to_wire()
            },
        )?;

        Ok(self.fetch::<WireLookupResponse>(requests)?)
    }

    async fn process_files_upload(
        &self,
        repo: String,
        data: Vec<(AnyFileContentId, Bytes)>,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadToken>, EdenApiError> {
        if data.is_empty() {
            return Ok(Response::empty());
        }
        // Filter already uploaded file contents first

        let mut uploaded_indices = HashSet::<usize>::new();
        let mut uploaded_tokens: Vec<UploadToken> = vec![];

        let anyids = data
            .iter()
            .map(|(id, _data)| AnyId::AnyFileContentId(id.clone()))
            .collect();

        let mut entries = self
            .lookup_batch(repo.clone(), anyids, bubble_id)
            .await?
            .entries;
        while let Some(entry) = entries.next().await {
            if let Ok(entry) = entry {
                if let Some(token) = entry.token {
                    uploaded_indices.insert(entry.index);
                    uploaded_tokens.push(token)
                }
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
                .enumerate()
                .filter(|(index, _)| !uploaded_indices.contains(index))
                .map(|(_, (id, content))| {
                    let repo = repo.clone();
                    async move {
                        self.process_single_file_upload(repo, id, content, bubble_id)
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
                    }
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
        repo: String,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let msg = format!("Requesting hg filenodes upload for {} item(s)", items.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_FILENODES, Some(&repo))?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_UPLOAD_FILENODES_PER_REQUEST),
            |ids| {
                let req = Batch::<_> {
                    batch: ids
                        .into_iter()
                        .map(|item| UploadHgFilenodeRequest { data: item })
                        .collect(),
                };
                req.to_wire()
            },
        )?;

        Ok(self.fetch::<WireUploadTokensResponse>(requests)?)
    }

    async fn upload_trees_batch(
        &self,
        repo: String,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, EdenApiError> {
        let msg = format!("Requesting trees upload for {} item(s)", items.len());
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if items.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_TREES, Some(&repo))?;
        let requests = self.prepare_requests(
            &url,
            items,
            Some(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST),
            |ids| {
                let req = Batch::<_> {
                    batch: ids
                        .into_iter()
                        .map(|item| UploadTreeRequest { entry: item })
                        .collect(),
                };
                req.to_wire()
            },
        )?;

        Ok(self.fetch::<WireUploadTreeResponse>(requests)?)
    }

    // the request isn't batched, batching should be done outside if needed
    async fn upload_changesets(
        &self,
        repo: String,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let msg = format!(
            "Requesting changesets upload for {} item(s)",
            changesets.len(),
        );
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        if changesets.is_empty() {
            return Ok(Response::empty());
        }

        let url = self.build_url(paths::UPLOAD_CHANGESETS, Some(&repo))?;
        let req = UploadHgChangesetsRequest {
            changesets,
            mutations,
        }
        .to_wire();

        let request = self
            .configure_request(Request::post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireUploadTokensResponse>(vec![request])?)
    }

    async fn upload_bonsai_changeset(
        &self,
        repo: String,
        changeset: BonsaiChangesetContent,
        bubble_id: Option<std::num::NonZeroU64>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let msg = "Requesting changeset upload";
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }

        let mut url = self.build_url(paths::UPLOAD_BONSAI_CHANGESET, Some(&repo))?;
        if let Some(bubble_id) = bubble_id {
            url.query_pairs_mut()
                .append_pair("bubble_id", &bubble_id.to_string());
        }
        let req = UploadBonsaiChangesetRequest { changeset }.to_wire();

        let request = self
            .configure_request(Request::post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireUploadTokensResponse>(vec![request])?)
    }

    async fn ephemeral_prepare(
        &self,
        repo: String,
    ) -> Result<Response<EphemeralPrepareResponse>, EdenApiError> {
        let msg = "Preparing ephemeral bubble";
        tracing::info!("{}", &msg);
        if self.config().debug {
            eprintln!("{}", &msg);
        }
        let url = self.build_url(paths::EPHEMERAL_PREPARE, Some(&repo))?;
        let req = EphemeralPrepareRequest {}.to_wire();
        let request = self
            .configure_request(Request::post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        let mut fetch = self.fetch::<WireEphemeralPrepareResponse>(vec![request])?;
        fetch.entries = fetch
            .entries
            .inspect_ok(|r| tracing::info!("Created bubble {}", r.bubble_id))
            .boxed();

        Ok(fetch)
    }

    async fn fetch_snapshot(
        &self,
        repo: String,
        request: FetchSnapshotRequest,
    ) -> Result<Response<FetchSnapshotResponse>, EdenApiError> {
        tracing::info!("Fetching snapshot {}", request.cs_id,);
        if self.config().debug {
            eprintln!("Fetching snapshot {}", request.cs_id);
        }
        let url = self.build_url(paths::FETCH_SNAPSHOT, Some(&repo))?;
        let req = request.to_wire();
        let request = self
            .configure_request(Request::post(url.clone()))?
            .cbor(&req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireFetchSnapshotResponse>(vec![request])?)
    }

    async fn download_file(&self, repo: String, token: UploadToken) -> Result<Bytes, EdenApiError> {
        let download_file = "Downloading file";
        tracing::info!("{}", download_file);
        if self.config().debug {
            eprintln!("{}", download_file);
        }
        let url = self.build_url(paths::DOWNLOAD_FILE, Some(&repo))?;
        let metadata = token.data.metadata.clone();
        let req = token.to_wire();
        let request = self
            .configure_request(Request::post(url.clone()))?
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

    let body = res.into_body().decoded().try_concat().await?;
    let mut message = String::from_utf8_lossy(&body).into_owned();

    if message.len() >= 9 && &*message[..9].to_lowercase() == "<!doctype" {
        message = "HTML content omitted (this error may have come from a proxy server)".into();
    } else if message.len() > MAX_ERROR_MSG_LEN {
        message.truncate(MAX_ERROR_MSG_LEN);
        message.push_str("... (truncated)")
    }

    Err(EdenApiError::HttpError { status, message })
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
        let url: String = client.build_url(path, Some(repo_name))?.into();
        let expected =
            "https://example.com/repo_-.%20%21%40%23%24%25%20foo%20%F0%9F%92%A9%20bar/path";
        assert_eq!(&url, &expected);

        Ok(())
    }
}
