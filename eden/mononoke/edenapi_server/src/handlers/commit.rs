/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Error};
use futures::{stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{Location, LocationToHash, LocationToHashRequest};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_api::hg::HgRepoContext;

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

pub async fn location_to_hash(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state);
    let params = LocationToHashParams::borrow_from(state);
    let hg_repo_ctx = get_repo(&sctx, &rctx, &params.repo).await?;

    let request: LocationToHashRequest = parse_cbor_request(state).await?;
    let hgid_list = request
        .locations
        .into_iter()
        .map(move |location| translate_location(hg_repo_ctx.clone(), location));
    let response = stream::iter(hgid_list).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
    Ok(cbor_stream(response))
}

async fn translate_location(
    hg_repo_ctx: HgRepoContext,
    location: Location,
) -> Result<LocationToHash, Error> {
    if location.count != 1 {
        return Err(
            format_err!("location to hash with count different that 1 is unimplemented")
                .context(ErrorKind::NotImplemented),
        );
    }
    let known_descendant = HgChangesetId::new(HgNodeHash::from(location.known_descendant));

    let ancestor: HgChangesetId = hg_repo_ctx
        .location_to_hg_changeset_id(known_descendant, location.distance_to_descendant)
        .await
        .with_context(|| ErrorKind::CommitLocationToHashRequestFailed)?;
    let answer = LocationToHash::new(location, ancestor.into_nodehash().into());

    Ok(answer)
}
