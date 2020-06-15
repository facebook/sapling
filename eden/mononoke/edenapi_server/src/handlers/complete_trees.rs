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

use edenapi_types::{CompleteTreeRequest, DataEntry};
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
pub struct CompleteTreesParams {
    repo: String,
}

pub async fn complete_trees(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = CompleteTreesParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request = parse_cbor_request(state).await?;

    Ok(cbor_stream(fetch_trees_under_path(&repo, request)?))
}

/// Fetch the complete tree under the specified path.
///
/// This function returns all tree nodes underneath (and including)
/// a given directory in the repo. Multiple versions of the
/// root directory can be specified (via their manifest IDs);
/// all tree nodes reachable from any of these root nodes will
/// be fetched.
///
/// Optionally, the caller can specify a list of versions of
/// the root directory that are already present on the client.
/// It is assumed that the client possess the *complete tree*
/// underneath each of these versions. Any tree node reachable
/// from any of these root nodes will not be fetched.
///
/// This is essentially an HTTP-based implementation of Mercurial's
/// `gettreepack` wire protocol command. This is generally considered
/// a fairly expensive way to request trees. When possible, clients
/// should prefer to request individual tree nodes as needed via the
/// more lightweight `/trees` endpoint.
fn fetch_trees_under_path(
    repo: &HgRepoContext,
    request: CompleteTreeRequest,
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
        .map_err(|e| e.context(ErrorKind::CompleteTreeRequestFailed))
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
