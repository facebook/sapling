/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use futures::stream;
use futures::try_join;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::response::TryIntoResponse;
use serde::Deserialize;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::time::Duration;

use blobstore::Loadable;
use edenapi_types::wire::WireCommitHashToLocationRequestBatch;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::BonsaiFileChange;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitGraphRequest;
use edenapi_types::CommitHashLookupRequest;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitId;
use edenapi_types::CommitIdScheme;
use edenapi_types::CommitLocationToHashRequest;
use edenapi_types::CommitLocationToHashRequestBatch;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitMutationsRequest;
use edenapi_types::CommitMutationsResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::CommitRevlogDataRequest;
use edenapi_types::CommitTranslateIdRequest;
use edenapi_types::CommitTranslateIdResponse;
use edenapi_types::EphemeralPrepareRequest;
use edenapi_types::EphemeralPrepareResponse;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::UploadBonsaiChangesetRequest;
use edenapi_types::UploadHgChangesetsRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokensResponse;
use ephemeral_blobstore::BubbleId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::Globalrev;
use tunables::tunables;
use types::HgId;
use types::Parents;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::cbor_stream_filtered_errors;
use crate::utils::custom_cbor_stream;
use crate::utils::get_repo;
use crate::utils::parse_cbor_request;
use crate::utils::parse_wire_request;
use crate::utils::to_create_change;
use crate::utils::to_hg_path;
use crate::utils::to_mononoke_path;
use crate::utils::to_revlog_changeset;

use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerInfo;
use super::HandlerResult;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;
const HASH_TO_LOCATION_BATCH_SIZE: usize = 100;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct HashToLocationParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct RevlogDataParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct UploadBonsaiChangesetQueryString {
    bubble_id: Option<NonZeroU64>,
}

pub struct LocationToHashHandler;

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

#[async_trait]
impl EdenApiHandler for LocationToHashHandler {
    type Request = CommitLocationToHashRequestBatch;
    type Response = CommitLocationToHashResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CommitLocationToHash;
    const ENDPOINT: &'static str = "/commit/location_to_hash";

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero_ext::nonzero!(100u64)
    }

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let hgid_list = request
            .requests
            .into_iter()
            .map(move |location| translate_location(repo.clone(), location));
        let response = stream::iter(hgid_list).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
        Ok(response.boxed())
    }
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
            let result = match hgcsid_to_location.as_ref() {
                Ok(hsh) => match hsh.get(&hgcsid) {
                    Some(Ok(l)) => Ok(Some(l.map_descendant(|x| x.into()))),
                    Some(Err(e)) => Err(e.into()),
                    None => Ok(None),
                },
                Err(e) => Err(e.into()),
            };
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

    ScubaMiddlewareState::try_set_sampling_rate(state, nonzero_ext::nonzero!(256_u64));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();

    let hg_repo_ctx = get_repo(sctx, &rctx, &params.repo, None).await?;

    let batch = parse_wire_request::<WireCommitHashToLocationRequestBatch>(state).await?;
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
        .flatten();
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

    let hg_repo_ctx = get_repo(sctx, &rctx, &params.repo, None).await?;

    let request: CommitRevlogDataRequest = parse_cbor_request(state).await?;
    let revlog_commits = request
        .hgids
        .into_iter()
        .map(move |hg_id| commit_revlog_data(hg_repo_ctx.clone(), hg_id));
    let response =
        stream::iter(revlog_commits).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST);
    Ok(cbor_stream_filtered_errors(response))
}

async fn commit_revlog_data(
    hg_repo_ctx: HgRepoContext,
    hg_id: HgId,
) -> Result<CommitRevlogData, Error> {
    let bytes = hg_repo_ctx
        .revlog_commit_data(hg_id.into())
        .await
        .context(ErrorKind::CommitRevlogDataRequestFailed)?
        .ok_or(ErrorKind::HgIdNotFound(hg_id))?;
    let answer = CommitRevlogData::new(hg_id, bytes);
    Ok(answer)
}

pub struct HashLookupHandler;

#[async_trait]
impl EdenApiHandler for HashLookupHandler {
    type Request = Batch<CommitHashLookupRequest>;
    type Response = CommitHashLookupResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CommitHashLookup;
    const ENDPOINT: &'static str = "/commit/hash_lookup";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        use CommitHashLookupRequest::*;
        Ok(stream::iter(request.batch.into_iter())
            .then(move |request| {
                let hg_repo_ctx = repo.clone();
                async move {
                    let changesets = match request {
                        InclusiveRange(low, high) => {
                            hg_repo_ctx.get_hg_in_range(low.into(), high.into()).await?
                        }
                    };
                    let hgids = changesets.into_iter().map(|x| x.into()).collect();
                    let response = CommitHashLookupResponse { request, hgids };
                    Ok(response)
                }
            })
            .boxed())
    }
}

/// Upload list of HgChangesets requested by the client
pub struct UploadHgChangesetsHandler;

#[async_trait]
impl EdenApiHandler for UploadHgChangesetsHandler {
    type Request = UploadHgChangesetsRequest;
    type Response = UploadTokensResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::UploadHgChangesets;
    const ENDPOINT: &'static str = "/upload/changesets";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let changesets = request.changesets;
        let mutations = request.mutations;
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
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?;

        let results = repo
            .store_hg_changesets(changesets_data, mutation_data)
            .await?
            .into_iter()
            .map(move |r| {
                r.map(|(hg_cs_id, _bonsai_cs_id)| {
                    let hgid = HgId::from(hg_cs_id.into_nodehash());
                    UploadTokensResponse {
                        token: UploadToken::new_fake_token(AnyId::HgChangesetId(hgid), None),
                    }
                })
                .map_err(Error::from)
            });

        Ok(stream::iter(results).boxed())
    }
}

/// Upload list of bonsai changesets requested by the client
pub struct UploadBonsaiChangesetHandler;

#[async_trait]
impl EdenApiHandler for UploadBonsaiChangesetHandler {
    type QueryStringExtractor = UploadBonsaiChangesetQueryString;
    type Request = UploadBonsaiChangesetRequest;
    type Response = UploadTokensResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::UploadBonsaiChangeset;
    const ENDPOINT: &'static str = "/upload/changeset/bonsai";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let bubble_id = query.bubble_id.map(BubbleId::new);
        let cs = request.changeset;
        let repo = &repo;
        let parents = stream::iter(cs.hg_parents)
            .then(|hgid| async move {
                repo.get_bonsai_from_hg(hgid.into())
                    .await?
                    .ok_or_else(|| anyhow!("Parent HgId {} is invalid", hgid))
            })
            .try_collect()
            .await?;
        let cs_id = repo
            .repo()
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
                        let create_change = to_create_change(fc, bubble_id)
                            .with_context(|| anyhow!("Parsing file changes for {}", path))?;
                        Ok((to_mononoke_path(path)?, create_change))
                    })
                    .collect::<anyhow::Result<_>>()?,
                match bubble_id {
                    Some(id) => Some(repo.open_bubble(id).await?),
                    None => None,
                }
                .as_ref(),
            )
            .await
            .with_context(|| anyhow!("When creating bonsai changeset"))?
            .id();

        Ok(stream::once(async move {
            Ok(UploadTokensResponse {
                token: UploadToken::new_fake_token(
                    AnyId::BonsaiChangesetId(cs_id.into()),
                    bubble_id.map(Into::into),
                ),
            })
        })
        .boxed())
    }
}

/// Get information about a snapshot changeset
pub struct FetchSnapshotHandler;

#[async_trait]
impl EdenApiHandler for FetchSnapshotHandler {
    type Request = FetchSnapshotRequest;
    type Response = FetchSnapshotResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::FetchSnapshot;
    const ENDPOINT: &'static str = "/snapshot";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = &repo;
        let cs_id = ChangesetId::from(request.cs_id);
        let bubble_id = repo
            .ephemeral_store()
            .bubble_from_changeset(&cs_id)
            .await?
            .context("Snapshot not in a bubble")?;
        let blobstore = repo.bubble_blobstore(Some(bubble_id)).await?;
        let cs = cs_id.load(repo.ctx(), &blobstore).await?.into_mut();
        let time = cs.author_date.timestamp_secs();
        let tz = cs.author_date.tz_offset_secs();
        let response = FetchSnapshotResponse {
            author: cs.author,
            time,
            tz,
            hg_parents: Parents::from_iter(
                stream::iter(
                    cs.parents
                        .into_iter()
                        .map(|cs_id| repo.get_hg_from_bonsai(cs_id)),
                )
                .buffered(2)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .map(|id| id.into()),
            ),
            file_changes: cs
                .file_changes
                .into_iter()
                .map(|(path, fc)| {
                    Ok((
                        to_hg_path(&path.into())?,
                        match fc {
                            FileChange::Deletion => BonsaiFileChange::Deletion,
                            FileChange::UntrackedDeletion => BonsaiFileChange::UntrackedDeletion,
                            FileChange::Change(tc) => BonsaiFileChange::Change {
                                upload_token: UploadToken::new_fake_token(
                                    AnyId::AnyFileContentId(AnyFileContentId::ContentId(
                                        tc.content_id().into(),
                                    )),
                                    Some(bubble_id.into()),
                                ),
                                file_type: tc.file_type().into(),
                            },
                            FileChange::UntrackedChange(uc) => BonsaiFileChange::UntrackedChange {
                                upload_token: UploadToken::new_fake_token(
                                    AnyId::AnyFileContentId(AnyFileContentId::ContentId(
                                        uc.content_id().into(),
                                    )),
                                    Some(bubble_id.into()),
                                ),
                                file_type: uc.file_type().into(),
                            },
                        },
                    ))
                })
                .collect::<Result<_, Error>>()?,
            bubble_id: Some(bubble_id.into()),
        };
        Ok(stream::once(async move { Ok(response) }).boxed())
    }
}

/// Creates an ephemeral bubble and return its id
pub struct EphemeralPrepareHandler;

#[async_trait]
impl EdenApiHandler for EphemeralPrepareHandler {
    type Request = EphemeralPrepareRequest;
    type Response = EphemeralPrepareResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::EphemeralPrepare;
    const ENDPOINT: &'static str = "/ephemeral/prepare";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        Ok(stream::once(async move {
            Ok(EphemeralPrepareResponse {
                bubble_id: repo
                    .create_bubble(request.custom_duration_secs.map(Duration::from_secs))
                    .await?
                    .bubble_id()
                    .into(),
            })
        })
        .boxed())
    }
}

pub struct GraphHandler;

#[async_trait]
impl EdenApiHandler for GraphHandler {
    type Request = CommitGraphRequest;
    type Response = CommitGraphEntry;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CommitGraph;
    const ENDPOINT: &'static str = "/commit/graph";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let heads = request
            .heads
            .into_iter()
            .map(|hg_id| HgChangesetId::new(HgNodeHash::from(hg_id)))
            .collect();
        let common = request
            .common
            .into_iter()
            .map(|hg_id| HgChangesetId::new(HgNodeHash::from(hg_id)))
            .collect();

        let graph_entries = repo
            .get_graph_mapping(common, heads)
            .await?
            .into_iter()
            .map(|(hgid, parents)| {
                Ok(CommitGraphEntry {
                    hgid: HgId::from(hgid.into_nodehash()),
                    parents: parents
                        .into_iter()
                        .map(|p_hgid| HgId::from(p_hgid.into_nodehash()))
                        .collect(),
                })
            });
        Ok(stream::iter(graph_entries).boxed())
    }
}

pub struct CommitMutationsHandler;

#[async_trait]
impl EdenApiHandler for CommitMutationsHandler {
    type Request = CommitMutationsRequest;
    type Response = CommitMutationsResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CommitMutations;
    const ENDPOINT: &'static str = "/commit/mutations";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        if !tunables().get_mutation_generate_for_draft() {
            return Ok(stream::empty().boxed());
        }
        let commits = request
            .commits
            .into_iter()
            .map(|hg_id| HgChangesetId::new(HgNodeHash::from(hg_id)))
            .collect();

        let mutations = repo
            .fetch_mutations(commits)
            .await?
            .into_iter()
            .map(|mutation| {
                Ok(CommitMutationsResponse {
                    mutation: mutation.into(),
                })
            });

        Ok(stream::iter(mutations).boxed())
    }
}

pub struct CommitTranslateId;

#[async_trait]
impl EdenApiHandler for CommitTranslateId {
    type Request = CommitTranslateIdRequest;
    type Response = CommitTranslateIdResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::CommitTranslateId;
    const ENDPOINT: &'static str = "/commit/translate_id";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let mut hg_ids = Vec::new();
        let mut bonsai_ids = Vec::new();
        let mut git_ids = Vec::new();
        let mut globalrevs = Vec::new();
        for commit in &request.commits {
            match commit {
                CommitId::Hg(hg_id) => hg_ids.push(HgChangesetId::from(*hg_id)),
                CommitId::Bonsai(bonsai_id) => bonsai_ids.push(ChangesetId::from(*bonsai_id)),
                CommitId::GitSha1(git_id) => git_ids.push(GitSha1::from(*git_id)),
                CommitId::Globalrev(globalrev) => globalrevs.push(Globalrev::new(*globalrev)),
            }
        }
        // Convert request types to bonsais
        let (hg_bonsais, git_bonsais, globalrev_bonsais) = try_join!(
            repo.repo().many_changeset_ids_from_hg(hg_ids),
            repo.repo().many_changeset_ids_from_git_sha1(git_ids),
            repo.repo().many_changeset_ids_from_globalrev(globalrevs),
        )?;
        let hg_bonsais = hg_bonsais.into_iter().collect::<HashMap<_, _>>();
        let git_bonsais = git_bonsais.into_iter().collect::<HashMap<_, _>>();
        let globalrev_bonsais = globalrev_bonsais.into_iter().collect::<HashMap<_, _>>();
        // Collect all bonsai changeset ids
        let all_bonsai_ids: Vec<ChangesetId> = bonsai_ids
            .into_iter()
            .chain(hg_bonsais.values().copied())
            .chain(git_bonsais.values().copied())
            .chain(globalrev_bonsais.values().copied())
            .collect();
        // Convert all bonsais to the target type
        let bonsai_translations: HashMap<ChangesetId, CommitId> = match request.scheme {
            CommitIdScheme::Bonsai => all_bonsai_ids
                .into_iter()
                .map(|id| (id.clone(), CommitId::Bonsai(id.clone().into())))
                .collect(),
            CommitIdScheme::Hg => repo
                .repo()
                .many_changeset_hg_ids(all_bonsai_ids)
                .await?
                .into_iter()
                .map(|(id, hg_id)| (id, CommitId::Hg(hg_id.into())))
                .collect(),
            CommitIdScheme::GitSha1 => repo
                .repo()
                .many_changeset_git_sha1s(all_bonsai_ids)
                .await?
                .into_iter()
                .map(|(id, git_sha1)| (id, CommitId::GitSha1(git_sha1.into())))
                .collect(),
            CommitIdScheme::Globalrev => repo
                .repo()
                .many_changeset_globalrev_ids(all_bonsai_ids)
                .await?
                .into_iter()
                .map(|(id, globalrev)| (id, CommitId::Globalrev(globalrev.id())))
                .collect(),
        };
        // Build the response based on the original request
        let translations: Vec<_> = request
            .commits
            .into_iter()
            .filter_map(|commit| {
                let id = match commit {
                    CommitId::Hg(hg_id) => *hg_bonsais.get(&HgChangesetId::from(hg_id))?,
                    CommitId::Bonsai(id) => ChangesetId::from(id),
                    CommitId::GitSha1(git_id) => *git_bonsais.get(&GitSha1::from(git_id))?,
                    CommitId::Globalrev(globalrev) => {
                        *globalrev_bonsais.get(&Globalrev::new(globalrev))?
                    }
                };
                let translated = bonsai_translations.get(&id)?.clone();
                Some(CommitTranslateIdResponse { commit, translated })
            })
            .map(anyhow::Ok)
            .collect();
        Ok(stream::iter(translations).boxed())
    }
}
