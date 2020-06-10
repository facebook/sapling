/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use futures::TryStreamExt;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{DataEntry, DataResponse, TreeRequest};
use gotham_ext::{error::HttpError, response::BytesBody};
use mercurial_types::{HgManifestId, HgNodeHash};
use mononoke_api::{
    hg::{HgDataContext, HgRepoContext, HgTreeContext},
    path::MononokePath,
};
use types::Key;

use crate::context::ServerContext;
use crate::middleware::RequestContext;

use super::util::{cbor_mime, get_repo, get_request_body, to_hg_path, to_mononoke_path};

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct SubTreeParams {
    repo: String,
}

pub async fn subtree(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = SubTreeParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let body = get_request_body(state).await?;

    let request = serde_cbor::from_slice(&body).map_err(HttpError::e400)?;
    let response = get_complete_subtree(&repo, request).await?;
    let bytes: Bytes = serde_cbor::to_vec(&response)
        .map_err(HttpError::e500)?
        .into();

    Ok(BytesBody::new(bytes, cbor_mime()))
}

/// Fetch all of the nodes for the subtree under the specified
/// path for the specified root node versions of this path.
/// The client may optionally specify a list of root versions
/// for the path that it already has, and any nodes in these
/// older subtrees will be filtered out if present in the
/// requested subtrees.
///
/// This is essentially an HTTP-based implementation of Mercurial's
/// `gettreepack` wire protocol command, and is generally considered
/// a fairly expensive way to request trees. When possible, clients
/// should prefer explicitly request individual tree nodes via the
/// more lightweight `/trees` endpoint.
async fn get_complete_subtree(
    repo: &HgRepoContext,
    request: TreeRequest,
) -> Result<DataResponse, HttpError> {
    let path = to_mononoke_path(request.rootdir).map_err(HttpError::e400)?;

    let root_nodes = request
        .mfnodes
        .into_iter()
        .map(|hgid| HgManifestId::new(HgNodeHash::from(hgid)))
        .collect::<Vec<_>>();

    let base_nodes = request
        .basemfnodes
        .into_iter()
        .map(|hgid| HgManifestId::new(HgNodeHash::from(hgid)))
        .collect::<Vec<_>>();

    let entries = repo
        .trees_under_path(path, root_nodes, base_nodes, request.depth)
        .map_err(HttpError::e500)
        .and_then(move |(tree, path)| async {
            // XXX: Even though this function isn't async, we need to
            // use an async block because `and_then()` requires a Future.
            data_entry_for_tree(tree, path).map_err(HttpError::e500)
        })
        // TODO(kulshrax): Change this method to return a stream
        // instead of buffering the data entires.
        .try_collect::<Vec<_>>()
        .await?;

    Ok(DataResponse::new(entries))
}

fn data_entry_for_tree(tree: HgTreeContext, path: MononokePath) -> Result<DataEntry, Error> {
    let hgid = tree.node_id().into_nodehash().into();
    let path = to_hg_path(&path)?;

    let key = Key::new(path, hgid);
    let data = tree.content_bytes();
    let parents = tree.hg_parents().into();

    Ok(DataEntry::new(key, data, parents))
}
