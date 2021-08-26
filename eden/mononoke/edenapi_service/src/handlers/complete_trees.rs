/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};

use edenapi_types::{CompleteTreeRequest, EdenApiServerError, TreeEntry};
use mercurial_types::{HgManifestId, HgNodeHash};
use mononoke_api::path::MononokePath;
use mononoke_api_hg::{HgDataContext, HgRepoContext, HgTreeContext};
use rate_limiting::Metric;
use types::Key;

use crate::errors::ErrorKind;
use crate::utils::{to_hg_path, to_mononoke_path};

use super::{EdenApiHandler, EdenApiMethod, HandlerError, HandlerResult};

pub struct CompleteTreesHandler;

#[async_trait]
impl EdenApiHandler for CompleteTreesHandler {
    type Request = CompleteTreeRequest;
    type Response = Result<TreeEntry, EdenApiServerError>;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CompleteTrees;
    const ENDPOINT: &'static str = "/trees/complete";

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
    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let ctx = repo.ctx().clone();

        let path = to_mononoke_path(request.rootdir).map_err(|e| HandlerError::E400(e))?;

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
            .map_err(|e| EdenApiServerError::new(e.context(ErrorKind::CompleteTreeRequestFailed)))
            .and_then(move |(tree, path)| async { entry_for_tree(tree, path) })
            .inspect_ok(move |_| {
                ctx.session().bump_load(Metric::TotalManifests, 1.0);
            })
            // Errors are actually stored inside the response object
            .map(|r| Ok(r));

        Ok(stream.boxed())
    }
}

fn entry_for_tree(
    tree: HgTreeContext,
    path: MononokePath,
) -> Result<TreeEntry, EdenApiServerError> {
    let hgid = tree.node_id().into_nodehash().into();
    let path = to_hg_path(&path).map_err(|e| EdenApiServerError::with_hgid(hgid, e))?;

    let key = Key::new(path, hgid);
    let data = tree.content_bytes();
    let parents = tree.hg_parents().into();

    Ok(TreeEntry::new(key, data, parents))
}
