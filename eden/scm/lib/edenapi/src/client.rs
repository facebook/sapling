/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::FromIterator;

use async_trait::async_trait;
use futures::prelude::*;
use itertools::Itertools;
use serde::{de::DeserializeOwned, Serialize};
use url::Url;

use edenapi_types::{
    CompleteTreeRequest, DataEntry, DataRequest, HistoryEntry, HistoryRequest, HistoryResponseChunk,
};
use http_client::{HttpClient, Request};
use types::{HgId, Key, RepoPathBuf};

use crate::api::{EdenApi, ProgressCallback};
use crate::builder::{ClientCreds, Config};
use crate::errors::EdenApiError;
use crate::name::RepoName;
use crate::response::{Fetch, ResponseMeta};

mod paths {
    pub const HEALTH_CHECK: &str = "health_check";
    pub const FILES: &str = "files";
    pub const HISTORY: &str = "history";
    pub const TREES: &str = "trees";
    pub const COMPLETE_TREES: &str = "trees/complete";
}

pub struct Client {
    config: Config,
    client: HttpClient,
}

impl Client {
    /// Create an EdenAPI client with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        Self {
            config,
            client: HttpClient::new(),
        }
    }

    /// Append a repo name and endpoint path onto the server's base URL.
    fn url(&self, path: &str, repo: Option<&RepoName>) -> Result<Url, EdenApiError> {
        let url = &self.config.server_url;
        Ok(match repo {
            Some(repo) => url.join(&format!("{}/", repo))?.join(path)?,
            None => url.join(path)?,
        })
    }

    /// Configure a request to use the client's configured TLS credentials.
    fn configure_tls(&self, mut req: Request) -> Result<Request, EdenApiError> {
        if let Some(ClientCreds { cert, key }) = &self.config.client_creds {
            req = req.creds(cert, key)?;
        }
        if let Some(ca) = &self.config.ca_bundle {
            req = req.cainfo(ca)?;
        }
        Ok(req)
    }

    /// Prepare a collection of POST requests for the given keys.
    /// The keys will be grouped into batches of the specified size and
    /// passed to the `make_req` callback, which should insert them into
    /// a struct that will be CBOR-encoded and used as the request body.
    fn prepare<K, F, R>(
        &self,
        url: &Url,
        keys: K,
        batch_size: Option<usize>,
        mut make_req: F,
    ) -> Result<Vec<Request>, EdenApiError>
    where
        K: IntoIterator<Item = Key>,
        F: FnMut(Vec<Key>) -> R,
        R: Serialize,
    {
        split_into_batches(keys, batch_size)
            .into_iter()
            .map(|keys| {
                let req = make_req(keys);
                self.configure_tls(Request::post(url.clone()))?
                    .cbor(&req)
                    .map_err(EdenApiError::RequestSerializationFailed)
            })
            .collect()
    }

    /// Fetch data from the server.
    ///
    /// Concurrently performs all of the given HTTP requests, each of
    /// which must result in streaming response of CBOR-encoded values
    /// of type `T`. The metadata of each response will be returned in
    /// the order the responses arrive. The response streams will be
    /// combined into a single stream, in which the returned entries
    /// from different HTTP responses may be arbitrarily interleaved.
    async fn fetch<T: DeserializeOwned + Send + 'static>(
        &self,
        requests: Vec<Request>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<T>, EdenApiError> {
        let progress = progress.unwrap_or_else(|| Box::new(|_| ()));
        let requests = requests.into_iter().collect::<Vec<_>>();
        let n_requests = requests.len();

        let (mut responses, stats) = self.client.send_async_with_progress(requests, progress)?;

        let mut meta = Vec::with_capacity(n_requests);
        let mut streams = Vec::with_capacity(n_requests);

        while let Some(res) = responses.try_next().await? {
            meta.push(ResponseMeta::from(&res));

            let entries = res.into_cbor_stream::<T>().err_into().boxed();
            streams.push(entries);
        }

        let entries = stream::select_all(streams).boxed();

        Ok(Fetch {
            meta,
            entries,
            stats,
        })
    }
}

#[async_trait]
impl EdenApi for Client {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        let url = self.url(paths::HEALTH_CHECK, None)?;
        let req = self.configure_tls(Request::get(url))?;
        let res = req.send_async().await?;
        Ok(ResponseMeta::from(&res))
    }

    async fn files(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        if keys.is_empty() {
            return Err(EdenApiError::EmptyRequest);
        }

        let url = self.url(paths::FILES, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_files, |keys| DataRequest {
            keys,
        })?;

        self.fetch::<DataEntry>(requests, progress).await
    }

    async fn history(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError> {
        if keys.is_empty() {
            return Err(EdenApiError::EmptyRequest);
        }

        let url = self.url(paths::HISTORY, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_history, |keys| {
            HistoryRequest { keys, length }
        })?;

        let Fetch {
            meta,
            entries,
            stats,
        } = self
            .fetch::<HistoryResponseChunk>(requests, progress)
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
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        if keys.is_empty() {
            return Err(EdenApiError::EmptyRequest);
        }

        let url = self.url(paths::TREES, Some(&repo))?;
        let requests = self.prepare(&url, keys, self.config.max_trees, |keys| DataRequest {
            keys,
        })?;

        self.fetch::<DataEntry>(requests, progress).await
    }

    async fn complete_trees(
        &self,
        repo: RepoName,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        let url = self.url(paths::COMPLETE_TREES, Some(&repo))?;
        let tree_req = CompleteTreeRequest {
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        };

        let req = self
            .configure_tls(Request::post(url))?
            .cbor(&tree_req)
            .map_err(EdenApiError::RequestSerializationFailed)?;

        self.fetch::<DataEntry>(vec![req], progress).await
    }
}

/// Split up a collection of keys into batches of at most `batch_size`.
fn split_into_batches(
    keys: impl IntoIterator<Item = Key>,
    batch_size: Option<usize>,
) -> Vec<Vec<Key>> {
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
