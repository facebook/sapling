/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use futures::{stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{
    CommitLocation, CommitLocationToHash, CommitLocationToHashRequest, CommitRevlogData,
    CommitRevlogDataRequest,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_api::hg::HgRepoContext;
use types::HgId;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct LocationToHashParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct RevlogDataParams {
    repo: String,
}

pub async fn location_to_hash(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state);
    let params = LocationToHashParams::borrow_from(state);
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo).await?;

    let request: CommitLocationToHashRequest = parse_cbor_request(state).await?;
    let hgid_list = request
        .locations
        .into_iter()
        .map(move |location| translate_location(hg_repo_ctx.clone(), location));
    let response = stream::iter(hgid_list).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
    Ok(cbor_stream(response))
}

pub async fn revlog_data(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state);
    let params = RevlogDataParams::borrow_from(state);
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo).await?;

    let request: CommitRevlogDataRequest = parse_cbor_request(state).await?;
    let revlog_commits = request
        .hgids
        .into_iter()
        .map(move |hg_id| commit_revlog_data(hg_repo_ctx.clone(), hg_id));
    let response =
        stream::iter(revlog_commits).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
    Ok(cbor_stream(response))
}

async fn translate_location(
    hg_repo_ctx: HgRepoContext,
    location: CommitLocation,
) -> Result<CommitLocationToHash, Error> {
    let known_descendant = HgChangesetId::new(HgNodeHash::from(location.known_descendant));

    let ancestors: Vec<HgChangesetId> = hg_repo_ctx
        .location_to_hg_changeset_id(
            known_descendant,
            location.distance_to_descendant,
            location.count,
        )
        .await
        .with_context(|| ErrorKind::CommitLocationToHashRequestFailed)?;
    let converted = ancestors.iter().map(|a| a.into_nodehash().into()).collect();
    let answer = CommitLocationToHash::new(location, converted);

    Ok(answer)
}

async fn commit_revlog_data(
    hg_repo_ctx: HgRepoContext,
    hg_id: HgId,
) -> Result<CommitRevlogData, Error> {
    let hg_cs_id = HgChangesetId::new(HgNodeHash::from(hg_id));
    let bytes = hg_repo_ctx
        .revlog_commit_data(hg_cs_id)
        .await
        .with_context(|| ErrorKind::CommitRevlogDataRequestFailed)?
        .ok_or_else(|| ErrorKind::HgIdNotFound(hg_id))?;
    let answer = CommitRevlogData::new(hg_id, bytes);
    Ok(answer)
}
