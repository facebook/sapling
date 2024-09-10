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
use edenapi_types::wire::WireTreeRequest;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::FileAuxData;
use edenapi_types::SaplingRemoteApiServerError;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeAuxData;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use edenapi_types::TreeRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeRequest;
use edenapi_types::UploadTreeResponse;
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
use gotham_ext::error::HttpError;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::response::TryIntoResponse;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgDataContext;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;
use mononoke_api_hg::HgTreeContext;
use rate_limiting::Metric;
use repo_blobstore::RepoBlobstoreRef;
use serde::Deserialize;
use types::Key;
use types::RepoPathBuf;

use super::handler::SaplingRemoteApiContext;
use super::HandlerInfo;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::request_dumper::RequestDumper;
use crate::utils::custom_cbor_stream;
use crate::utils::get_repo;
use crate::utils::parse_wire_request;

// The size is optimized for the batching settings in EdenFs.
const MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST: usize = 128;
const MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH: usize = 100;
const MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST: usize = 100;
const LARGE_TREE_METADATA_LIMIT: usize = 25000;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct TreeParams {
    repo: String,
}

/// Fetch the tree nodes requested by the client.
pub async fn trees(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = TreeParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        SaplingRemoteApiMethod::Trees,
    ));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo: HgRepoContext<Repo> =
        get_repo(sctx, &rctx, &params.repo, Metric::TotalManifests).await?;
    let request = parse_wire_request::<WireTreeRequest>(state).await?;
    if let Some(rd) = RequestDumper::try_borrow_mut_from(state) {
        rd.add_request(&request);
    };

    ScubaMiddlewareState::try_set_sampling_rate(state, nonzero_ext::nonzero!(256_u64));

    Ok(custom_cbor_stream(
        super::monitor_request(state, fetch_all_trees(repo, request)),
        |tree_entry| tree_entry.as_ref().err(),
    ))
}

/// Fetch trees for all of the requested keys concurrently.
fn fetch_all_trees<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    request: TreeRequest,
) -> impl Stream<Item = Result<TreeEntry, SaplingRemoteApiServerError>> {
    let ctx = repo.ctx().clone();

    let fetches = request.keys.into_iter().map(move |key| {
        fetch_tree(repo.clone(), key.clone(), request.attributes)
            .map(|r| r.map_err(|e| SaplingRemoteApiServerError::with_key(key, e)))
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
async fn fetch_tree<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    key: Key,
    attributes: TreeAttributes,
) -> Result<TreeEntry, Error> {
    let mut entry = TreeEntry::new(key.clone());

    if attributes.augmented_trees {
        let id = HgAugmentedManifestId::new(HgNodeHash::from(key.hgid));
        repo.ctx()
            .perf_counters()
            .increment_counter(PerfCounterType::EdenapiAugmentedTrees);

        let maybe_ctx = id
            .context(repo.clone())
            .await
            .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

        if let Some(ctx) = maybe_ctx {
            entry.with_tree_aux_data(TreeAuxData {
                augmented_manifest_id: ctx.augmented_manifest_id().clone().into(),
                augmented_manifest_size: ctx.augmented_manifest_size(),
            });

            if attributes.parents {
                entry.with_parents(Some(ctx.hg_parents().into()));
            }

            if attributes.child_metadata {
                repo.ctx()
                    .perf_counters()
                    .increment_counter(PerfCounterType::EdenapiTreesAuxData);

                entry.with_children(Some(
                    ctx.augmented_children_entries()
                        .map(|(path, augmented_entry)| match augmented_entry {
                            HgAugmentedManifestEntry::FileNode(file) => {
                                Ok(TreeChildEntry::new_file_entry(
                                    Key {
                                        hgid: file.filenode.into(),
                                        path: RepoPathBuf::from_string(path.to_string()).map_err(
                                            |e| {
                                                SaplingRemoteApiServerError::with_key(
                                                    key.clone(),
                                                    e,
                                                )
                                            },
                                        )?,
                                    },
                                    FileAuxData {
                                        blake3: file.content_blake3.clone().into(),
                                        sha1: file.content_sha1.clone().into(),
                                        total_size: file.total_size.clone(),
                                        file_header_metadata: Some(
                                            file.file_header_metadata
                                                .clone()
                                                .unwrap_or(Bytes::new())
                                                .into(),
                                        ),
                                    }
                                    .into(),
                                ))
                            }
                            HgAugmentedManifestEntry::DirectoryNode(tree) => {
                                Ok(TreeChildEntry::new_directory_entry(
                                    Key {
                                        hgid: tree.treenode.into(),
                                        path: RepoPathBuf::from_string(path.to_string()).map_err(
                                            |e| {
                                                SaplingRemoteApiServerError::with_key(
                                                    key.clone(),
                                                    e,
                                                )
                                            },
                                        )?,
                                    },
                                    TreeAuxData {
                                        augmented_manifest_id: tree
                                            .augmented_manifest_id
                                            .clone()
                                            .into(),
                                        augmented_manifest_size: tree
                                            .augmented_manifest_size
                                            .clone(),
                                    },
                                ))
                            }
                        })
                        .collect(),
                ));
            }

            if attributes.manifest_blob {
                let (data, _) = ctx
                    .content()
                    .await
                    .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

                entry.with_data(Some(data.into()));
            }

            return Ok(entry);
        } else {
            // If we don't have an augmented tree, fallback to the old way of fetching trees
            // Log the fallback to scuba
            repo.ctx()
                .perf_counters()
                .increment_counter(PerfCounterType::EdenapiAugmentedTreesFallback);
        }
    }

    let id = <HgManifestId as HgDataId<R>>::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo.clone())
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    if attributes.manifest_blob {
        repo.ctx()
            .perf_counters()
            .increment_counter(PerfCounterType::EdenapiTrees);

        let (data, _) = ctx
            .content()
            .await
            .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

        entry.with_data(Some(data.into()));
    }

    if attributes.parents {
        entry.with_parents(Some(ctx.hg_parents().into()));
    }

    if attributes.child_metadata {
        repo.ctx()
            .perf_counters()
            .increment_counter(PerfCounterType::EdenapiTreesAuxData);

        if let Some(entries) = fetch_child_file_metadata_entries(&repo, &ctx).await? {
            let children: Vec<Result<TreeChildEntry, SaplingRemoteApiServerError>> = entries
                .buffer_unordered(MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH)
                .map(|r| r.map_err(|e| SaplingRemoteApiServerError::with_key(key.clone(), e)))
                .collect()
                .await;

            entry.with_children(Some(children));
        }
    }

    Ok(entry)
}

async fn fetch_child_file_metadata_entries<'a, R: MononokeRepo>(
    repo: &'a HgRepoContext<R>,
    ctx: &'a HgTreeContext<R>,
) -> Result<
    Option<impl Stream<Item = impl Future<Output = Result<TreeChildEntry, Error>> + 'a> + 'a>,
    Error,
> {
    let manifest = ctx.clone().into_blob_manifest()?;
    if manifest.content().files.len() > LARGE_TREE_METADATA_LIMIT {
        return Ok(None);
    }
    let file_entries = manifest
        .list(repo.ctx(), repo.repo().repo_blobstore())
        .await?
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .filter_map(|(name, entry)| {
            if let Entry::Leaf((_, child_id)) = entry {
                Some((name, child_id))
            } else {
                None
            }
        });

    Ok(Some(
        stream::iter(file_entries)
            // .entries iterator is not `Send`
            .map({
                move |(name, child_id)| async move {
                    let name = RepoPathBuf::from_string(name.to_string())?;
                    let child_key = Key::new(name, child_id.into_nodehash().into());
                    fetch_child_file_metadata(repo, child_key.clone()).await
                }
            }),
    ))
}

async fn fetch_child_file_metadata<R: MononokeRepo>(
    repo: &HgRepoContext<R>,
    child_key: Key,
) -> Result<TreeChildEntry, Error> {
    let ctx = repo
        .file(HgFileNodeId::new(child_key.hgid.into()))
        .await?
        .ok_or_else(|| ErrorKind::FileFetchFailed(child_key.clone()))?;

    let metadata = ctx.content_metadata().await?;
    Ok(TreeChildEntry::new_file_entry(
        child_key,
        FileAuxData {
            total_size: metadata.total_size,
            sha1: metadata.sha1.into(),
            blake3: metadata.seeded_blake3.into(),
            file_header_metadata: Some(ctx.file_header_metadata().into()),
        }
        .into(),
    ))
}

/// Store the content of a single tree
async fn store_tree<R: MononokeRepo>(
    repo: HgRepoContext<R>,
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
impl SaplingRemoteApiHandler for UploadTreesHandler {
    type Request = Batch<UploadTreeRequest>;
    type Response = UploadTreeResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::UploadTrees;
    const ENDPOINT: &'static str = "/upload/trees";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let tokens = request
            .batch
            .into_iter()
            .map(move |item| store_tree(repo.clone(), item));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST)
            .boxed())
    }
}
