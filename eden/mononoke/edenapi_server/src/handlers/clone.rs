/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use futures::stream::{self, StreamExt, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::wire::{ToWire, WireCloneData, WireIdMapEntry};
use gotham_ext::content::ContentStream;
use gotham_ext::error::HttpError;
use gotham_ext::response::{BytesBody, StreamBody, TryIntoResponse};
use gotham_ext::stream_ext::GothamTryStreamExt;
use types::HgId;

use crate::context::ServerContext;
use crate::errors::MononokeErrorExt;
use crate::handlers::{EdenApiMethod, HandlerInfo};
use crate::middleware::RequestContext;
use crate::utils::{cbor, get_repo};

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct CloneParams {
    repo: String,
}

pub async fn clone_data(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = CloneParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Clone));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo).await?;
    // Note that we have CloneData<HgChangesetId> which doesn't have a direct to wire conversion.
    // This means that we need to manually construct WireCloneData for all the WireHgId entries.
    let clone_data = hg_repo_ctx
        .segmented_changelog_clone_data()
        .await
        .map_err(|e| e.into_http_error("error getting segmented changelog data"))?;
    let idmap = clone_data
        .idmap
        .into_iter()
        .map(|(k, v)| WireIdMapEntry {
            dag_id: k.to_wire(),
            hg_id: HgId::from(v.into_nodehash()).to_wire(),
        })
        .collect();
    let wire_clone_data = WireCloneData {
        head_id: clone_data.head_id.to_wire(),
        flat_segments: clone_data.flat_segments.segments.to_wire(),
        idmap,
    };

    Ok(BytesBody::new(
        cbor::to_cbor_bytes(wire_clone_data).map_err(HttpError::e500)?,
        cbor::cbor_mime(),
    ))
}

pub async fn full_idmap_clone_data(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = CloneParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Clone));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo).await?;
    // Note that we have CloneData<HgChangesetId> which doesn't have a direct to wire conversion.
    // This means that we need to manually construct WireCloneData for all the WireHgId entries.
    let clone_data = hg_repo_ctx
        .segmented_changelog_full_idmap_clone_data()
        .await
        .map_err(|e| e.into_http_error("error getting segmented changelog data"))?;

    // Experimenting here (hacks). The full IdMap is large. We are going to construct WireCloneData
    // without the idmap and send that over the wire. Next on the wire we are going to stream
    // WireIdMapEntry objects.  The receiver is expected to deserialize a WireCloneData object then
    // continue to read idmap entries from the body of the response.
    let iddag_clone_data = WireCloneData {
        head_id: clone_data.head_id.to_wire(),
        flat_segments: clone_data.flat_segments.segments.to_wire(),
        idmap: Vec::new(),
    };
    let iddag_byte_stream = stream::iter(vec![cbor::to_cbor_bytes(iddag_clone_data)]);
    let idmap_byte_stream = clone_data.idmap_stream.and_then(|(k, v)| async move {
        let entry = WireIdMapEntry {
            dag_id: k.to_wire(),
            hg_id: HgId::from(v.into_nodehash()).to_wire(),
        };
        cbor::to_cbor_bytes(entry)
    });

    let byte_stream = iddag_byte_stream.chain(idmap_byte_stream);
    let content_stream = ContentStream::new(byte_stream).forward_err(rctx.error_tx);

    Ok(StreamBody::new(content_stream, cbor::cbor_mime()))
}
