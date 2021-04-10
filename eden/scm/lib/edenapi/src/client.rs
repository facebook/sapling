/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::{create_dir_all, File};
use std::iter::FromIterator;

use anyhow::{format_err, Context};
use async_trait::async_trait;
use futures::prelude::*;
use itertools::Itertools;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_cbor::Deserializer;
use url::Url;

use auth::check_certs;
use edenapi_types::{
    json::ToJson,
    wire::{
        WireCloneData, WireCommitHashToLocationResponse, WireCommitLocationToHashResponse,
        WireFileEntry, WireHistoryResponseChunk, WireIdMapEntry, WireToApiConversionError,
        WireTreeEntry,
    },
    CloneData, CommitHashToLocationRequestBatch, CommitHashToLocationResponse,
    CommitLocationToHashRequest, CommitLocationToHashRequestBatch, CommitLocationToHashResponse,
    CommitRevlogData, CommitRevlogDataRequest, CompleteTreeRequest, EdenApiServerError, FileEntry,
    FileRequest, HistoryEntry, HistoryRequest, ToApi, ToWire, TreeAttributes, TreeEntry,
    TreeRequest,
};
use hg_http::http_client;
use http_client::{AsyncResponse, HttpClient, HttpClientError, Progress, Request};
use types::{HgId, Key, RepoPathBuf};

use crate::api::{EdenApi, ProgressCallback};
use crate::builder::Config;
use crate::errors::EdenApiError;
use crate::response::{Fetch, ResponseMeta};

/// All non-alphanumeric characters (except hypens, underscores, and periods)
/// found in the repo's name will be percent-encoded before being used in URLs.
const RESERVED_CHARS: &AsciiSet = &NON_ALPHANUMERIC.remove(b'_').remove(b'-').remove(b'.');

mod paths {
    pub const HEALTH_CHECK: &str = "health_check";
    pub const FILES: &str = "files";
    pub const HISTORY: &str = "history";
    pub const TREES: &str = "trees";
    pub const COMPLETE_TREES: &str = "trees/complete";
    pub const COMMIT_REVLOG_DATA: &str = "commit/revlog_data";
    pub const CLONE_DATA: &str = "clone";
    pub const FULL_IDMAP_CLONE_DATA: &str = "full_idmap_clone";
    pub const COMMIT_LOCATION_TO_HASH: &str = "commit/location_to_hash";
    pub const COMMIT_HASH_TO_LOCATION: &str = "commit/hash_to_location";
}

pub struct Client {
    config: Config,
    client: HttpClient,
}

impl Client {
    /// Create an EdenAPI client with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        let client = http_client("edenapi").verbose(config.debug);
        Self { config, client }
    }

    /// Append a repo name and endpoint path onto the server's base URL.
    fn url(&self, path: &str, repo: Option<&str>) -> Result<Url, EdenApiError> {
        let url = &self.config.server_url;
        Ok(match repo {
            Some(repo) => url
                // Repo name must be sanitized since it can be set by the user.
                .join(&format!("{}/", utf8_percent_encode(repo, RESERVED_CHARS)))?
                .join(path)?,
            None => url.join(path)?,
        })
    }

    /// Add configured values to a request.
    fn configure(&self, mut req: Request) -> Result<Request, EdenApiError> {
        if let Some(ref cert) = self.config.cert {
            if self.config.validate_certs {
                check_certs(cert)?;
            }
            req.set_cert(cert);
        }

        if let Some(ref key) = self.config.key {
            req.set_key(key);
        }

        if let Some(ref ca) = self.config.ca_bundle {
            req.set_cainfo(ca);
        }

        for (k, v) in &self.config.headers {
            req.set_header(k, v);
        }

        if let Some(ref correlator) = self.config.correlator {
            req.set_header("X-Client-Correlator", correlator);
        }

        if let Some(timeout) = self.config.timeout {
            req.set_timeout(timeout);
        }

        if let Some(http_version) = self.config.http_version {
            req.set_http_version(http_version);
        }

        Ok(req)
    }

    /// Prepare a collection of POST requests for the given keys.
    /// The keys will be grouped into batches of the specified size and
    /// passed to the `make_req` callback, which should insert them into
    /// a struct that will be CBOR-encoded and used as the request body.
    fn prepare<T, K, F, R>(
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
                self.configure(Request::post(url.clone()))?
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
    async fn fetch_raw<T: DeserializeOwned + Send + 'static>(
        &self,
        requests: Vec<Request>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<T>, EdenApiError> {
        let progress = progress.unwrap_or_else(|| Box::new(|_| ()));
        let n_requests = requests.len();

        let (mut responses, stats) = self.client.send_async_with_progress(requests, progress)?;

        let mut meta = Vec::with_capacity(n_requests);
        let mut streams = Vec::with_capacity(n_requests);

        while let Some(res) = responses.try_next().await? {
            let res = raise_for_status(res).await?;

            let response_meta = ResponseMeta::from(&res);
            tracing::debug!("{:?}", &response_meta);
            meta.push(response_meta);

            let entries = res.into_cbor_stream::<T>().err_into().boxed();
            streams.push(entries);
        }

        let entries = stream::select_all(streams).boxed();
        let stats = stats.err_into().boxed();

        Ok(Fetch {
            meta,
            entries,
            stats,
        })
    }

    /// Fetch data from the server.
    ///
    /// Concurrently performs all of the given HTTP requests, each of
    /// which must result in streaming response of CBOR-encoded values
    /// of type `T`. The metadata of each response will be returned in
    /// the order the responses arrive. The response streams will be
    /// combined into a single stream, in which the returned entries
    /// from different HTTP responses may be arbitrarily interleaved.
    async fn fetch<T>(
        &self,
        requests: Vec<Request>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<<T as ToApi>::Api>, EdenApiError>
    where
        T: ToApi + Send + DeserializeOwned + 'static,
        <T as ToApi>::Api: Send + 'static,
    {
        let Fetch {
            meta,
            entries,
            stats,
        } = self.fetch_raw::<T>(requests, progress).await?;

        let entries = entries
            .and_then(|v| future::ready(v.to_api().map_err(|e| EdenApiError::from(e.into()))))
            .boxed();

        Ok(Fetch {
            meta,
            entries,
            stats,
        })
    }

    /// Log the request to the configured log directory as JSON.
    fn log_request<R: ToJson + Debug>(&self, req: &R, label: &str) {
        tracing::trace!("Sending request: {:?}", req);

        let log_dir = match &self.config.log_dir {
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
}

#[async_trait]
impl EdenApi for Client {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        let url = self.url(paths::HEALTH_CHECK, None)?;

        let msg = format!("Sending health check request: {}", &url);
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        let req = self.configure(Request::get(url))?;
        let res = req.send_async().await?;

        Ok(ResponseMeta::from(&res))
    }

    async fn files(
        &self,
        repo: String,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<FileEntry>, EdenApiError> {
        let msg = format!("Requesting content for {} file(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Fetch::empty());
        }

        let url = self.url(paths::FILES, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_files, |keys| {
            let req = FileRequest { keys };
            self.log_request(&req, "files");
            req.to_wire()
        })?;

        Ok(self.fetch::<WireFileEntry>(requests, progress).await?)
    }

    async fn history(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError> {
        let msg = format!("Requesting history for {} file(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Fetch::empty());
        }

        let url = self.url(paths::HISTORY, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_history, |keys| {
            let req = HistoryRequest { keys, length };
            self.log_request(&req, "history");
            req.to_wire()
        })?;

        let Fetch {
            meta,
            entries,
            stats,
        } = self
            .fetch::<WireHistoryResponseChunk>(requests, progress)
            .await?;

        // Convert received `HistoryResponseChunk`s into `HistoryEntry`s.
        let entries = entries
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten()
            .boxed();

        Ok(Fetch {
            meta,
            entries,
            stats,
        })
    }

    async fn trees(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let msg = format!("Requesting {} trees(s)", keys.len());
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        if keys.is_empty() {
            return Ok(Fetch::empty());
        }

        let url = self.url(paths::TREES, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_trees, |keys| {
            let req = TreeRequest {
                keys,
                attributes: attributes.clone().unwrap_or_default(),
            };
            self.log_request(&req, "trees");
            req.to_wire()
        })?;

        Ok(self.fetch::<WireTreeEntry>(requests, progress).await?)
    }

    async fn complete_trees(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let msg = format!(
            "Requesting {} complete tree(s) for directory '{}'",
            mfnodes.len(),
            &rootdir
        );
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        let url = self.url(paths::COMPLETE_TREES, Some(&repo))?;
        let tree_req = CompleteTreeRequest {
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        };
        self.log_request(&tree_req, "complete_trees");
        let wire_tree_req = tree_req.to_wire();

        let req = self
            .configure(Request::post(url))?
            .cbor(&wire_tree_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        Ok(self.fetch::<WireTreeEntry>(vec![req], progress).await?)
    }

    async fn commit_revlog_data(
        &self,
        repo: String,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitRevlogData>, EdenApiError> {
        let msg = format!("Requesting revlog data for {} commit(s)", hgids.len());
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        let url = self.url(paths::COMMIT_REVLOG_DATA, Some(&repo))?;
        let commit_revlog_data_req = CommitRevlogDataRequest { hgids };

        self.log_request(&commit_revlog_data_req, "commit_revlog_data");

        let req = self
            .configure(Request::post(url))?
            .cbor(&commit_revlog_data_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch_raw::<CommitRevlogData>(vec![req], progress)
            .await
    }

    async fn clone_data(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let msg = format!("Requesting clone data for the '{}' repository", repo);
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        let url = self.url(paths::CLONE_DATA, Some(&repo))?;
        let req = self.configure(Request::post(url))?;
        let mut fetch = self.fetch::<WireCloneData>(vec![req], progress).await?;
        let clone_data = fetch.entries.next().await.ok_or_else(|| {
            EdenApiError::Other(format_err!("clone data missing from reponse body"))
        })??;
        Ok(clone_data)
    }

    async fn full_idmap_clone_data(
        &self,
        repo: String,
        mut progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let msg = format!(
            "Requesting full idmap clone data for the '{}' repository",
            repo
        );
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        let url = self.url(paths::FULL_IDMAP_CLONE_DATA, Some(&repo))?;
        let req = self.configure(Request::post(url))?;
        let async_response = req
            .send_async()
            .await
            .context("error receiving async response")?;
        let response_bytes = async_response
            .body
            .try_fold(Vec::new(), |mut acc, v| {
                if let Some(callback) = &mut progress {
                    // strictly speaking not correct because it does not count overhead
                    callback(Progress::new(v.len(), acc.len() + v.len(), 0, 0));
                }
                acc.extend(v);
                future::ok(acc)
            })
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
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitLocationToHashResponse>, EdenApiError> {
        let msg = format!(
            "Requesting commit location to hash (batch size = {})",
            requests.len()
        );
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        if requests.is_empty() {
            return Ok(Fetch::empty());
        }

        let url = self.url(paths::COMMIT_LOCATION_TO_HASH, Some(&repo))?;

        let formatted = self.prepare(
            &url,
            requests,
            self.config.max_location_to_hash,
            |requests| {
                let batch = CommitLocationToHashRequestBatch { requests };
                self.log_request(&batch, "commit_location_to_hash");
                batch.to_wire()
            },
        )?;

        Ok(self
            .fetch::<WireCommitLocationToHashResponse>(formatted, progress)
            .await?)
    }

    async fn commit_hash_to_location(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitHashToLocationResponse>, EdenApiError> {
        let msg = format!(
            "Requesting commit hash to location (batch size = {})",
            hgids.len()
        );
        tracing::info!("{}", &msg);
        if self.config.debug {
            eprintln!("{}", &msg);
        }

        if hgids.is_empty() {
            return Ok(Fetch::empty());
        }

        let url = self.url(paths::COMMIT_HASH_TO_LOCATION, Some(&repo))?;

        let formatted = self.prepare(&url, hgids, self.config.max_location_to_hash, |hgids| {
            let batch = CommitHashToLocationRequestBatch {
                master_heads: master_heads.clone(),
                hgids,
            };
            self.log_request(&batch, "commit_hash_to_location");
            batch.to_wire()
        })?;

        Ok(self
            .fetch::<WireCommitHashToLocationResponse>(formatted, progress)
            .await?)
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
    if res.status.as_u16() < 400 {
        return Ok(res);
    }

    let body = res.body.try_concat().await?;
    let message = String::from_utf8_lossy(&body).into_owned();
    Err(EdenApiError::HttpError {
        status: res.status,
        message,
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::builder::Builder;

    #[test]
    fn test_url_escaping() -> Result<()> {
        let base_url = "https://example.com".parse()?;
        let client = Builder::new().server_url(base_url).build()?;

        let repo = "repo_-. !@#$% foo \u{1f4a9} bar";
        let path = "path";

        let url = client.url(path, Some(repo))?.into_string();
        let expected =
            "https://example.com/repo_-.%20%21%40%23%24%25%20foo%20%F0%9F%92%A9%20bar/path";
        assert_eq!(&url, &expected);

        Ok(())
    }
}
