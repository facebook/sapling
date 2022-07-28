/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use context::PerfCounterType;
use futures::stream;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use serde::Deserialize;

use edenapi_types::wire::WireTreeRequest;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::EdenApiServerError;
use edenapi_types::FileMetadata;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use edenapi_types::TreeRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeRequest;
use edenapi_types::UploadTreeResponse;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::response::TryIntoResponse;
use manifest::Entry;
use mercurial_types::FileType;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api_hg::HgDataContext;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;
use mononoke_api_hg::HgTreeContext;
use rate_limiting::Metric;
use types::Key;
use types::RepoPathBuf;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::custom_cbor_stream;
use crate::utils::get_repo;
use crate::utils::parse_wire_request;

use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerInfo;
use super::HandlerResult;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST: usize = 10;
const MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH: usize = 100;
const MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST: usize = 100;

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

    let repo = get_repo(sctx, &rctx, &params.repo, Metric::TotalManifests).await?;
    let request = parse_wire_request::<WireTreeRequest>(state).await?;
    repo.ctx()
        .perf_counters()
        .add_to_counter(PerfCounterType::EdenapiTrees, request.keys.len() as i64);

    ScubaMiddlewareState::try_set_sampling_rate(state, nonzero_ext::nonzero!(256_u64));

    Ok(custom_cbor_stream(
        fetch_all_trees(repo, request),
        |tree_entry| tree_entry.as_ref().err(),
    ))
}

/// Fetch trees for all of the requested keys concurrently.
fn fetch_all_trees(
    repo: HgRepoContext,
    request: TreeRequest,
) -> impl Stream<Item = Result<TreeEntry, EdenApiServerError>> {
    let ctx = repo.ctx().clone();

    let fetch_metadata = request.attributes.child_metadata;
    let fetches = request.keys.into_iter().map(move |key| {
        fetch_tree(repo.clone(), key.clone(), fetch_metadata)
            .map(|r| r.map_err(|e| EdenApiServerError::with_key(key, e)))
    });

    stream::iter(fetches)
        .buffer_unordered(MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST)
        .inspect_ok(move |_| {
            ctx.session().bump_load(Metric::TotalManifests, 1.0);
        })
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

    let (data, _) = ctx
        .content()
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;
    let parents = ctx.hg_parents().into();

    let mut entry = TreeEntry::new(key.clone(), data, parents);

    if fetch_metadata {
        let children: Vec<Result<TreeChildEntry, EdenApiServerError>> =
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
) -> Result<impl Stream<Item = impl Future<Output = Result<TreeChildEntry, Error>> + 'a> + 'a, Error>
{
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
                    Entry::Tree(child_id) => TreeChildEntry::new_directory_entry(Key::new(
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
) -> Result<TreeChildEntry, Error> {
    let fsnode = repo
        .file(HgFileNodeId::new(child_key.hgid.into()))
        .await?
        .ok_or_else(|| ErrorKind::FileFetchFailed(child_key.clone()))?
        .fetch_fsnode_data(file_type)
        .await?;
    Ok(TreeChildEntry::new_file_entry(
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

/// Store the content of a single tree
async fn store_tree(
    repo: HgRepoContext,
    item: UploadTreeRequest,
) -> Result<UploadTreeResponse, Error> {
    let upload_node_id = HgNodeHash::from(item.entry.node_id);
    let contents = item.entry.data;
    let p1 = item.entry.parents.p1().cloned().map(HgNodeHash::from);
    let p2 = item.entry.parents.p2().cloned().map(HgNodeHash::from);
    repo.store_tree(upload_node_id, p1, p2, Bytes::from(contents))
        .await?;
    Ok(UploadTreeResponse {
        token: UploadToken::new_fake_token(AnyId::HgTreeId(item.entry.node_id), None),
    })
}

/// Upload list of trees requested by the client (batch request).
pub struct UploadTreesHandler;

#[async_trait]
impl EdenApiHandler for UploadTreesHandler {
    type Request = Batch<UploadTreeRequest>;
    type Response = UploadTreeResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::UploadTrees;
    const ENDPOINT: &'static str = "/upload/trees";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let tokens = request
            .batch
            .into_iter()
            .map(move |item| store_tree(repo.clone(), item));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST)
            .boxed())
    }
}
