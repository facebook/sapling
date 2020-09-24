/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use futures::{stream, Stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{
    wire::{ToApi, ToWire, WireTreeRequest},
    TreeEntry, TreeRequest,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgManifestId, HgNodeHash};
use mononoke_api::hg::{HgDataContext, HgDataId, HgRepoContext};
use types::Key;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request};

use super::{EdenApiMethod, HandlerInfo};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST: usize = 10;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct TreeParams {
    repo: String,
}

/// Fetch the tree nodes requested by the client.
pub async fn trees(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = TreeParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Trees));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request: WireTreeRequest = parse_cbor_request(state).await?;
    let request: TreeRequest = match request.to_api() {
        Ok(r) => r,
        Err(e) => {
            return Err(HttpError::e400(e));
        }
    };

    Ok(cbor_stream(
        rctx,
        fetch_all_trees(repo, request).map(|r| r.map(|v| v.to_wire())),
    ))
}

/// Fetch trees for all of the requested keys concurrently.
fn fetch_all_trees(
    repo: HgRepoContext,
    request: TreeRequest,
) -> impl Stream<Item = Result<TreeEntry, Error>> {
    let fetches = request
        .keys
        .into_iter()
        .map(move |key| fetch_tree(repo.clone(), key));

    stream::iter(fetches).buffer_unordered(MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST)
}

/// Fetch requested tree for a single key.
/// Note that this function consumes the repo context in order
/// to construct a tree context for the requested blob.
async fn fetch_tree(repo: HgRepoContext, key: Key) -> Result<TreeEntry, Error> {
    let id = HgManifestId::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo)
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    let (data, metadata) = ctx
        .content()
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;
    let parents = ctx.hg_parents().into();

    Ok(TreeEntry::new(key, data, parents, metadata))
}
