/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Context, Error};
use futures::{stream, Stream, StreamExt, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;
use std::collections::BTreeMap;

use edenapi_types::{
    wire::{
        WireBatch, WireCommitHashLookupRequest, WireCommitHashToLocationRequestBatch,
        WireCommitLocationToHashRequestBatch, WireEphemeralPrepareRequest,
        WireUploadBonsaiChangesetRequest, WireUploadHgChangesetsRequest,
    },
    AnyId, CommitHashLookupRequest, CommitHashLookupResponse, CommitHashToLocationResponse,
    CommitLocationToHashRequest, CommitLocationToHashResponse, CommitRevlogData,
    CommitRevlogDataRequest, EphemeralPrepareRequest, EphemeralPrepareResponse, ToWire,
    UploadBonsaiChangesetRequest, UploadHgChangesetsRequest, UploadToken, UploadTokensResponse,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_api_hg::HgRepoContext;
use mononoke_types::DateTime;
use types::HgId;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{
    cbor_stream_filtered_errors, custom_cbor_stream, get_repo, parse_cbor_request,
    parse_wire_request, to_create_change, to_mononoke_path, to_mutation_entry, to_revlog_changeset,
};

use super::{EdenApiMethod, HandlerInfo};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;
const HASH_TO_LOCATION_BATCH_SIZE: usize = 100;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct LocationToHashParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct HashToLocationParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct RevlogDataParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct HashLookupParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct UploadHgChangesetsParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct UploadBonsaiChangesetParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct EphemeralPrepareParams {
    repo: String,
}

pub async fn location_to_hash(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = LocationToHashParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::CommitLocationToHash,
    ));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();

    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let batch = parse_wire_request::<WireCommitLocationToHashRequestBatch>(state).await?;
    let hgid_list = batch
        .requests
        .into_iter()
        .map(move |location| translate_location(hg_repo_ctx.clone(), location));
    let response = stream::iter(hgid_list)
        .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
        .map_ok(|response| response.to_wire());
    Ok(cbor_stream_filtered_errors(response))
}

pub async fn hash_to_location(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    async fn hash_to_location_chunk(
        hg_repo_ctx: HgRepoContext,
        master_heads: Vec<HgChangesetId>,
        hg_cs_ids: Vec<HgChangesetId>,
    ) -> impl Stream<Item = CommitHashToLocationResponse> {
        let hgcsid_to_location = hg_repo_ctx
            .many_changeset_ids_to_locations(master_heads, hg_cs_ids.clone())
            .await;
        let responses = hg_cs_ids.into_iter().map(move |hgcsid| {
            let result = hgcsid_to_location
                .as_ref()
                .map(|hsh| hsh.get(&hgcsid).map(|l| l.map_descendant(|x| x.into())))
                .map_err(|e| (&*e).into());
            CommitHashToLocationResponse {
                hgid: hgcsid.into(),
                result,
            }
        });
        stream::iter(responses)
    }

    let params = HashToLocationParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::CommitHashToLocation,
    ));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();

    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let batch = parse_wire_request::<WireCommitHashToLocationRequestBatch>(state).await?;
    let unfiltered = batch.unfiltered;
    let master_heads = batch
        .master_heads
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<_>>();

    let response = stream::iter(batch.hgids)
        .chunks(HASH_TO_LOCATION_BATCH_SIZE)
        .map(|chunk| chunk.into_iter().map(|x| x.into()).collect::<Vec<_>>())
        .map({
            let ctx = hg_repo_ctx.clone();
            move |chunk| hash_to_location_chunk(ctx.clone(), master_heads.clone(), chunk)
        })
        .buffer_unordered(3)
        .flatten()
        .filter(move |v| {
            // The old behavior is to filter out error and None results. We want to preserve that
            // behavior for old clients since they will not be able to deserialize other results.
            let to_keep = if unfiltered == Some(true) {
                true
            } else {
                match v.result {
                    Ok(Some(_)) => true,
                    _ => false,
                }
            };
            futures::future::ready(to_keep)
        });
    let cbor_response = custom_cbor_stream(response, |t| t.result.as_ref().err());
    Ok(cbor_response)
}

pub async fn revlog_data(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = RevlogDataParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::CommitRevlogData,
    ));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();

    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let request: CommitRevlogDataRequest = parse_cbor_request(state).await?;
    let revlog_commits = request
        .hgids
        .into_iter()
        .map(move |hg_id| commit_revlog_data(hg_repo_ctx.clone(), hg_id));
    let response =
        stream::iter(revlog_commits).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
    Ok(cbor_stream_filtered_errors(response))
}

pub async fn hash_lookup(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    use CommitHashLookupRequest::*;
    let params = HashLookupParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::CommitHashLookup,
    ));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();

    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let batch_request = parse_wire_request::<WireBatch<WireCommitHashLookupRequest>>(state).await?;
    let stream = stream::iter(batch_request.batch.into_iter()).then(move |request| {
        let hg_repo_ctx = hg_repo_ctx.clone();
        async move {
            let changesets = match request {
                InclusiveRange(low, high) => {
                    hg_repo_ctx.get_hg_in_range(low.into(), high.into()).await?
                }
            };
            let hgids = changesets.into_iter().map(|x| x.into()).collect();
            let response = CommitHashLookupResponse { request, hgids };
            Ok(response.to_wire())
        }
    });

    Ok(cbor_stream_filtered_errors(stream))
}

async fn translate_location(
    hg_repo_ctx: HgRepoContext,
    request: CommitLocationToHashRequest,
) -> Result<CommitLocationToHashResponse, Error> {
    let location = request.location.map_descendant(|x| x.into());
    let ancestors: Vec<HgChangesetId> = hg_repo_ctx
        .location_to_hg_changeset_id(location, request.count)
        .await
        .context(ErrorKind::CommitLocationToHashRequestFailed)?;
    let hgids = ancestors.into_iter().map(|x| x.into()).collect();
    let answer = CommitLocationToHashResponse {
        location: request.location,
        count: request.count,
        hgids,
    };
    Ok(answer)
}

async fn commit_revlog_data(
    hg_repo_ctx: HgRepoContext,
    hg_id: HgId,
) -> Result<CommitRevlogData, Error> {
    let bytes = hg_repo_ctx
        .revlog_commit_data(hg_id.into())
        .await
        .context(ErrorKind::CommitRevlogDataRequestFailed)?
        .ok_or_else(|| ErrorKind::HgIdNotFound(hg_id))?;
    let answer = CommitRevlogData::new(hg_id, bytes);
    Ok(answer)
}

/// Store list of HgChangesets
async fn store_hg_changesets(
    repo: HgRepoContext,
    request: UploadHgChangesetsRequest,
) -> Result<Vec<Result<UploadTokensResponse, Error>>, Error> {
    let changesets = request.changesets;
    let mutations = request.mutations;
    let indexes = changesets
        .iter()
        .enumerate()
        .map(|(index, cs)| (cs.node_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let changesets_data = changesets
        .into_iter()
        .map(|changeset| {
            Ok((
                HgChangesetId::new(HgNodeHash::from(changeset.node_id)),
                to_revlog_changeset(changeset.changeset_content)?,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let mutation_data = mutations
        .into_iter()
        .map(to_mutation_entry)
        .collect::<Result<Vec<_>, Error>>()?;

    let results = repo
        .store_hg_changesets(changesets_data, mutation_data)
        .await?
        .into_iter()
        .map(|r| {
            r.map(|(hg_cs_id, _bonsai_cs_id)| {
                let hgid = HgId::from(hg_cs_id.into_nodehash());
                UploadTokensResponse {
                    index: indexes.get(&hgid).cloned().unwrap(), // always present
                    token: UploadToken::new_fake_token(AnyId::HgChangesetId(hgid)),
                }
            })
            .map_err(Error::from)
        })
        .collect();

    Ok(results)
}

/// Upload list of HgChangesets requested by the client
pub async fn upload_hg_changesets(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = UploadHgChangesetsParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::UploadHgChangesets,
    ));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;
    let request = parse_wire_request::<WireUploadHgChangesetsRequest>(state).await?;
    let responses = store_hg_changesets(repo, request)
        .await
        .map_err(HttpError::e500)?;

    Ok(cbor_stream_filtered_errors(
        stream::iter(responses).map(|r| r.map(|v| v.to_wire())),
    ))
}
/// Store list of HgChangesets
async fn upload_bonsai_changeset_impl(
    repo: HgRepoContext,
    request: UploadBonsaiChangesetRequest,
) -> Result<Vec<Result<UploadTokensResponse, Error>>, Error> {
    let cs = request.changeset;
    let repo_write = repo.clone().write().await?;
    let repo = &repo;
    let parents = stream::iter(cs.hg_parents)
        .then(|hgid| async move {
            repo.get_bonsai_from_hg(hgid.into())
                .await?
                .ok_or_else(|| anyhow!("Parent HgId {} is invalid", hgid))
        })
        .try_collect()
        .await?;
    let cs_id = repo_write
        .create_changeset(
            parents,
            cs.author,
            DateTime::from_timestamp(cs.time, cs.tz)?.into(),
            None,
            None,
            cs.message,
            cs.extra.into_iter().map(|e| (e.key, e.value)).collect(),
            cs.file_changes
                .into_iter()
                .map(|(path, fc)| {
                    let create_change = to_create_change(fc)
                        .with_context(|| anyhow!("Parsing file changes for {}", path))?;
                    Ok((to_mononoke_path(path)?, create_change))
                })
                .collect::<anyhow::Result<_>>()?,
        )
        .await
        .with_context(|| anyhow!("When creating bonsai changeset"))?
        .id();

    Ok(vec![Ok(UploadTokensResponse {
        index: 0,
        token: UploadToken::new_fake_token(AnyId::BonsaiChangesetId(cs_id.into())),
    })])
}

/// Upload list of bonsai changesets requested by the client
pub async fn upload_bonsai_changeset(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = UploadBonsaiChangesetParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::UploadBonsaiChangeset,
    ));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;
    let request = parse_wire_request::<WireUploadBonsaiChangesetRequest>(state).await?;
    let responses = upload_bonsai_changeset_impl(repo, request)
        .await
        .map_err(HttpError::e500)?;

    Ok(cbor_stream_filtered_errors(
        stream::iter(responses).map(|r| r.map(|v| v.to_wire())),
    ))
}

async fn ephemeral_prepare_impl(
    repo: HgRepoContext,
    _request: EphemeralPrepareRequest,
) -> Result<EphemeralPrepareResponse, Error> {
    Ok(EphemeralPrepareResponse {
        bubble_id: repo.create_bubble().await?.bubble_id().into(),
    })
}

// Creates an ephemeral bubble and return its id
pub async fn ephemeral_prepare(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = EphemeralPrepareParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        EdenApiMethod::EphemeralPrepare,
    ));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;
    let request = parse_wire_request::<WireEphemeralPrepareRequest>(state).await?;
    let response = ephemeral_prepare_impl(repo, request)
        .await
        .map_err(HttpError::e500)?;

    Ok(cbor_stream_filtered_errors(stream::once(async move {
        Ok(response.to_wire())
    })))
}
