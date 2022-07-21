/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use serde::Deserialize;

use edenapi_types::wire::ToWire;
use edenapi_types::wire::WireCloneData;
use edenapi_types::wire::WireIdMapEntry;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use types::HgId;

use crate::context::ServerContext;
use crate::errors::MononokeErrorExt;
use crate::handlers::EdenApiMethod;
use crate::handlers::HandlerInfo;
use crate::middleware::RequestContext;
use crate::utils::cbor;
use crate::utils::get_repo;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct CloneParams {
    repo: String,
}

pub async fn clone_data(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = CloneParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Clone));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx = get_repo(sctx, &rctx, &params.repo, None).await?;
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
        flat_segments: clone_data
            .flat_segments
            .segments
            .into_iter()
            .collect::<Vec<_>>()
            .to_wire(),
        idmap,
    };

    Ok(BytesBody::new(
        cbor::to_cbor_bytes(&wire_clone_data).map_err(HttpError::e500)?,
        cbor::cbor_mime(),
    ))
}
