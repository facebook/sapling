/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::{Stream, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{DataEntry, TreeRequest};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgManifestId, HgNodeHash};
use mononoke_api::{
    hg::{HgDataContext, HgRepoContext, HgTreeContext},
    path::MononokePath,
};
use types::Key;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request, to_hg_path, to_mononoke_path};

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct SubTreeParams {
    repo: String,
}

pub async fn subtree(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = SubTreeParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request = parse_cbor_request(state).await?;

    Ok(cbor_stream(get_complete_subtree(&repo, request)?))
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
fn get_complete_subtree(
    repo: &HgRepoContext,
    request: TreeRequest,
) -> Result<impl Stream<Item = Result<DataEntry, Error>>, HttpError> {
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

    let stream = repo
        .trees_under_path(path, root_nodes, base_nodes, request.depth)
        .err_into::<Error>()
        .map_err(|e| e.context(ErrorKind::SubtreeRequestFailed))
        .and_then(move |(tree, path)| async { data_entry_for_tree(tree, path) });

    Ok(stream)
}

fn data_entry_for_tree(tree: HgTreeContext, path: MononokePath) -> Result<DataEntry, Error> {
    let hgid = tree.node_id().into_nodehash().into();
    let path = to_hg_path(&path)?;

    let key = Key::new(path, hgid);
    let data = tree.content_bytes();
    let parents = tree.hg_parents().into();

    Ok(DataEntry::new(key, data, parents))
}
