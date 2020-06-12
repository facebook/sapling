/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;

use anyhow::anyhow;
use futures::{
    stream::{BoxStream, FuturesUnordered},
    StreamExt, TryStreamExt,
};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{HistoryRequest, HistoryResponse, HistoryResponseChunk, WireHistoryEntry};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgFileNodeId, HgNodeHash};
use mononoke_api::hg::HgRepoContext;
use types::Key;

use crate::context::ServerContext;
use crate::middleware::RequestContext;
use crate::utils::{cbor_response, get_repo, parse_cbor_request, to_mononoke_path};

type HistoryStream = BoxStream<'static, Result<WireHistoryEntry, HttpError>>;

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
    let response = get_history(&repo, request).await?;

    cbor_response(response)
}

/// Fetch history for all of the requested files concurrently.
async fn get_history(
    repo: &HgRepoContext,
    request: HistoryRequest,
) -> Result<HistoryResponse, HttpError> {
    let chunk_stream = FuturesUnordered::new();
    for key in request.keys {
        // Build a stream of history entries for a single file.
        let entry_stream = single_key_history(repo, &key, request.length).await?;

        // Build a future that buffers the stream and resolves
        // to a HistoryResponseChunk for this file.
        let chunk_fut = async {
            Ok(HistoryResponseChunk {
                path: key.path,
                entries: entry_stream.try_collect().await?,
            })
        };

        chunk_stream.push(chunk_fut);
    }

    // TODO(kulshrax): Don't buffer the results here.
    let chunks = chunk_stream.try_collect().await?;
    let response = HistoryResponse { chunks };

    Ok(response)
}

async fn single_key_history(
    repo: &HgRepoContext,
    key: &Key,
    length: Option<u32>,
) -> Result<HistoryStream, HttpError> {
    let filenode_id = HgFileNodeId::new(HgNodeHash::from(key.hgid));
    let path = to_mononoke_path(&key.path).map_err(HttpError::e400)?;
    let mpath = path.into_mpath().ok_or_else(|| {
        HttpError::e400(anyhow!("empty path given for filenode: {}", &filenode_id))
    })?;

    let file = repo
        .file(filenode_id)
        .await
        .map_err(HttpError::e500)?
        .ok_or_else(|| HttpError::e404(anyhow!("file not found: {:?}", &key)))?;

    // Fetch the file's history and convert the entries into
    // the expected on-the-wire format.
    let history = file
        .history(mpath, length)
        .map_err(HttpError::e500)
        // XXX: Use async block because TryStreamExt::and_then
        // requires the closure to return a TryFuture.
        .and_then(|entry| async { WireHistoryEntry::try_from(entry).map_err(HttpError::e500) })
        .boxed();

    Ok(history)
}
