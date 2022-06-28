/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;

use cloned::cloned;
use edenapi_types::HistoryRequest;
use edenapi_types::HistoryResponseChunk;
use edenapi_types::WireHistoryEntry;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mononoke_api_hg::HgRepoContext;
use types::Key;

use crate::errors::ErrorKind;
use crate::utils::to_mpath;

use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerResult;

type HistoryStream = BoxStream<'static, Result<WireHistoryEntry, Error>>;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 10;

pub struct HistoryHandler;

#[async_trait]
impl EdenApiHandler for HistoryHandler {
    type Request = HistoryRequest;
    type Response = HistoryResponseChunk;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::History;
    const ENDPOINT: &'static str = "/history";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let HistoryRequest { keys, length } = request;

        let fetches = keys.into_iter().map(move |key| {
            // Construct a Future that buffers the full history for this key.
            // This should be OK since the history entries are relatively
            // small, so unless the history is extremely long, the total
            // amount of buffered data should be reasonable.
            cloned!(repo);
            async move {
                let path = key.path.clone();
                let stream = fetch_history_for_key(repo, key, length).await?;
                let entries = stream.try_collect().await?;
                Ok(HistoryResponseChunk { path, entries })
            }
        });

        Ok(stream::iter(fetches)
            .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
            .boxed())
    }
}

async fn fetch_history_for_key(
    repo: HgRepoContext,
    key: Key,
    length: Option<u32>,
) -> Result<HistoryStream, Error> {
    let filenode_id = HgFileNodeId::new(HgNodeHash::from(key.hgid));
    let mpath = to_mpath(&key.path)?.context(ErrorKind::UnexpectedEmptyPath)?;

    let file = repo
        .file(filenode_id)
        .await
        .with_context(|| ErrorKind::FileFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    // Fetch the file's history and convert the entries into
    // the expected on-the-wire format.
    let history = file
        .history(mpath, length)
        .err_into::<Error>()
        .map_err(move |e| e.context(ErrorKind::HistoryFetchFailed(key.clone())))
        .and_then(|entry| async { WireHistoryEntry::try_from(entry) })
        .boxed();

    Ok(history)
}
