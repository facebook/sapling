/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;

use anyhow::{Context, Error};
use futures::{
    stream::{self, BoxStream},
    Stream, StreamExt, TryStreamExt,
};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use cloned::cloned;
use edenapi_types::{HistoryRequest, HistoryResponseChunk, WireHistoryEntry};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgFileNodeId, HgNodeHash};
use mononoke_api::hg::HgRepoContext;
use types::Key;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request, to_mpath};

type HistoryStream = BoxStream<'static, Result<WireHistoryEntry, Error>>;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 10;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct HistoryParams {
    repo: String,
}

pub async fn history(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = HistoryParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request = parse_cbor_request(state).await?;

    Ok(cbor_stream(fetch_history(repo, request).await))
}

/// Fetch history for all of the requested files concurrently.
async fn fetch_history(
    repo: HgRepoContext,
    request: HistoryRequest,
) -> impl Stream<Item = Result<HistoryResponseChunk, Error>> {
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

    stream::iter(fetches).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
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
