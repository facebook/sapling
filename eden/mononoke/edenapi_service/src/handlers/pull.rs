/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::wire::{ToWire, WireCloneData, WireIdMapEntry};
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use types::HgId;

use crate::context::ServerContext;
use crate::errors::MononokeErrorExt;
use crate::handlers::{EdenApiMethod, HandlerInfo};
use crate::middleware::RequestContext;
use crate::utils::{cbor, get_repo, parse_wire_request};
use edenapi_types::wire::pull::WirePullFastForwardRequest;
use mercurial_types::HgChangesetId;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct PullFastForwardParams {
    repo: String,
}

pub async fn pull_fast_forward_master(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = PullFastForwardParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::PullFastForwardMaster,
    ));
    let request = parse_wire_request::<WirePullFastForwardRequest>(state).await?;

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let old_master: HgChangesetId = request.old_master.into();
    let new_master: HgChangesetId = request.new_master.into();
    let clone_data = hg_repo_ctx
        .segmented_changelog_pull_data(vec![old_master], vec![new_master])
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
