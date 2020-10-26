/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use futures::{stream, Future, FutureExt, Stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{
    wire::{ToApi, ToWire, WireTreeRequest},
    EdenApiServerError, FileMetadata, TreeEntry, TreeRequest,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use manifest::Entry;
use mercurial_types::{FileType, HgFileNodeId, HgManifestId, HgNodeHash};
use mononoke_api::hg::{HgDataContext, HgDataId, HgRepoContext, HgTreeContext};
use types::{Key, RepoPathBuf};

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request};

use super::{EdenApiMethod, HandlerInfo};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST: usize = 10;
const MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH: usize = 100;

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
        fetch_all_trees(repo, request).map(|r| Ok(r.to_wire())),
    ))
}

/// Fetch trees for all of the requested keys concurrently.
fn fetch_all_trees(
    repo: HgRepoContext,
    request: TreeRequest,
) -> impl Stream<Item = Result<TreeEntry, EdenApiServerError>> {
    let fetch_metadata = request.with_file_metadata.is_some();
    let fetches = request.keys.into_iter().map(move |key| {
        fetch_tree(repo.clone(), key.clone(), fetch_metadata)
            .map(|r| r.map_err(|e| EdenApiServerError::with_key(key, e)))
    });

    stream::iter(fetches).buffer_unordered(MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST)
}

/// Fetch requested tree for a single key.
/// Note that this function consumes the repo context in order
/// to construct a tree context for the requested blob.
async fn fetch_tree(
    repo: HgRepoContext,
    key: Key,
    fetch_metadata: bool,
) -> Result<TreeEntry, Error> {
    let id = HgManifestId::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo.clone())
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    let (data, metadata) = ctx
        .content()
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;
    let parents = ctx.hg_parents().into();

    let mut entry = TreeEntry::new(key.clone(), data, parents, metadata);

    if fetch_metadata {
        let children: Vec<Result<TreeEntry, EdenApiServerError>> =
            fetch_child_metadata_entries(&repo, &ctx)
                .await?
                .buffer_unordered(MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH)
                .map(|r| r.map_err(|e| EdenApiServerError::with_key(key.clone(), e)))
                .collect()
                .await;

        entry.with_children(Some(children));
    }

    Ok(entry)
}

async fn fetch_child_metadata_entries<'a>(
    repo: &'a HgRepoContext,
    ctx: &'a HgTreeContext,
) -> Result<impl Stream<Item = impl Future<Output = Result<TreeEntry, Error>> + 'a> + 'a, Error> {
    let entries = ctx.entries()?.collect::<Vec<_>>();

    Ok(stream::iter(entries)
        // .entries iterator is not `Send`
        .map({
            move |(name, entry)| async move {
                let name = RepoPathBuf::from_string(name.to_string())?;
                Ok(match entry {
                    Entry::Leaf((file_type, child_id)) => {
                        let child_key = Key::new(name, child_id.into_nodehash().into());
                        fetch_child_file_metadata(repo, file_type, child_key.clone()).await?
                    }
                    Entry::Tree(child_id) => TreeEntry::new_directory_entry(Key::new(
                        name,
                        child_id.into_nodehash().into(),
                    )),
                })
            }
        }))
}

async fn fetch_child_file_metadata(
    repo: &HgRepoContext,
    file_type: FileType,
    child_key: Key,
) -> Result<TreeEntry, Error> {
    let fsnode = repo
        .file(HgFileNodeId::new(child_key.hgid.into()))
        .await?
        .ok_or_else(|| ErrorKind::FileFetchFailed(child_key.clone()))?
        .fetch_fsnode_data(file_type)
        .await?;
    Ok(TreeEntry::new_file_entry(
        child_key,
        FileMetadata {
            file_type: Some((*fsnode.file_type()).into()),
            size: Some(fsnode.size()),
            content_sha1: Some((*fsnode.content_sha1()).into()),
            content_sha256: Some((*fsnode.content_sha256()).into()),
            content_id: Some((*fsnode.content_id()).into()),
            ..Default::default()
        },
    ))
}
