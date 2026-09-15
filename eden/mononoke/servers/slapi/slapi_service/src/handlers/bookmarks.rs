/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::format_err;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Freshness;
use bookmarks::MirrorBookmarkMove;
use bytes::Bytes;
use edenapi_types::BookmarkEntry;
use edenapi_types::BookmarkResult;
use edenapi_types::CODE_BOOKMARK_MOVE_ALREADY_PROCESSED;
use edenapi_types::HgId;
use edenapi_types::MirrorBookmarkMove as WireMirrorBookmarkMove;
use edenapi_types::MirrorBookmarkUpdateReason;
use edenapi_types::ReplayIdenticalMovesRequest;
use edenapi_types::ReplayIdenticalMovesResponse;
use edenapi_types::ServerError;
use edenapi_types::SetBookmarkRequest;
use edenapi_types::SetBookmarkResponse;
use edenapi_types::bookmark::Bookmark2Request;
use futures::StreamExt;
use futures::stream;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;
use crate::errors::ErrorKind;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;

/// Resolve the bookmarks requested by the client
pub struct Bookmarks2Handler;

/// Fetch the value of a single bookmark.
async fn fetch_bookmark<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    bookmark: String,
    flavour: SlapiCommitIdentityScheme,
    freshness: Freshness,
) -> Result<BookmarkEntry, Error> {
    let hgid = match flavour {
        SlapiCommitIdentityScheme::Git => repo
            .resolve_bookmark_git(bookmark.clone(), freshness)
            .await
            .map_err(|e| ErrorKind::BookmarkResolutionFailed(bookmark.clone(), e.into()))?
            .map(|id| HgId::from_slice(id.as_ref()))
            .transpose()?,
        SlapiCommitIdentityScheme::Hg => repo
            .resolve_bookmark(bookmark.clone(), freshness)
            .await
            .map_err(|e| ErrorKind::BookmarkResolutionFailed(bookmark.clone(), e.into()))?
            .map(|id| HgId::from(id.into_nodehash())),
    };

    Ok(BookmarkEntry { bookmark, hgid })
}

/// Create, delete, or move a bookmark
pub struct SetBookmarkHandler;

#[async_trait]
impl SaplingRemoteApiHandler for SetBookmarkHandler {
    type Request = SetBookmarkRequest;
    type Response = SetBookmarkResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::SetBookmark;
    const ENDPOINT: &'static str = "/bookmarks/set";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let res = set_bookmark_response(
            ectx.repo(),
            request.bookmark,
            request.to,
            request.from,
            request
                .pushvars
                .into_iter()
                .map(|p| (p.key, p.value.into()))
                .collect(),
        );

        Ok(stream::once(res).boxed())
    }

    fn extract_in_band_error(response: &Self::Response) -> Option<anyhow::Error> {
        response
            .data
            .as_ref()
            .err()
            .map(|err| format_err!("{err:?}"))
    }
}

async fn set_bookmark_response<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    bookmark: String,
    to: Option<HgId>,
    from: Option<HgId>,
    pushvars: HashMap<String, Bytes>,
) -> anyhow::Result<SetBookmarkResponse> {
    Ok(SetBookmarkResponse {
        data: set_bookmark(repo, bookmark, to, from, pushvars)
            .await
            .map_err(|e| ServerError::generic(format!("{e:?}"))),
    })
}

async fn set_bookmark<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    bookmark: String,
    to: Option<HgId>,
    from: Option<HgId>,
    pushvars: HashMap<String, Bytes>,
) -> Result<(), Error> {
    let repo = repo.repo_ctx();

    let pushvars = if pushvars.is_empty() {
        None
    } else {
        Some(&pushvars)
    };

    Ok(match (to, from) {
        (Some(to_hgid), Some(from_hgid)) => {
            // Move bookmark
            let to = HgChangesetId::new(HgNodeHash::from(to_hgid));
            let from = HgChangesetId::new(HgNodeHash::from(from_hgid));
            let (to, from) = futures::try_join!(
                async {
                    anyhow::Ok(
                        repo.changeset(to)
                            .await
                            .context("failed to resolve 'to' hgid")?
                            .ok_or(ErrorKind::HgIdNotFound(to_hgid))?
                            .id(),
                    )
                },
                async {
                    anyhow::Ok(
                        repo.changeset(from)
                            .await
                            .context("failed to resolve 'from' hgid")?
                            .ok_or(ErrorKind::HgIdNotFound(from_hgid))?
                            .id(),
                    )
                },
            )?;

            repo.move_bookmark(
                &BookmarkKey::new(&bookmark)?,
                to,
                Some(from),
                true,
                pushvars,
                None,
            )
            .await?
        }
        (Some(to_hgid), None) => {
            // Create bookmark
            let to = HgChangesetId::new(HgNodeHash::from(to_hgid));
            let to = repo
                .changeset(to)
                .await
                .context("failed to resolve 'to' hgid")?
                .ok_or(ErrorKind::HgIdNotFound(to_hgid))?
                .id();

            repo.create_bookmark(&BookmarkKey::new(&bookmark)?, to, pushvars, None)
                .await?
        }
        (None, Some(from_hgid)) => {
            // Delete bookmark
            let from = HgChangesetId::new(HgNodeHash::from(from_hgid));
            let from = repo
                .changeset(from)
                .await
                .context("failed to resolve 'from' hgid")?
                .ok_or(ErrorKind::HgIdNotFound(from_hgid))?
                .id();

            repo.delete_bookmark(&BookmarkKey::new(&bookmark)?, Some(from), pushvars)
                .await?
        }
        (None, None) => {
            return Err(Error::msg(
                "invalid SetBookmarkRequest, must specify at least one of 'to' or 'from'",
            ));
        }
    })
}

/// Mirror a contiguous chain of source bookmark moves to a `*_shadow` replica.
/// modern_sync uses this to keep the replica's bookmark and
/// bookmarks_update_log identical to the source, row for row.
pub struct ReplayIdenticalMovesHandler;

#[async_trait]
impl SaplingRemoteApiHandler for ReplayIdenticalMovesHandler {
    type Request = ReplayIdenticalMovesRequest;
    type Response = ReplayIdenticalMovesResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::ReplayIdenticalMoves;
    const ENDPOINT: &'static str = "/bookmarks/replay_identical_moves";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let res = replay_identical_moves_response(
            ectx.repo(),
            request.bookmark,
            request.moves,
            request
                .pushvars
                .into_iter()
                .map(|p| (p.key, p.value.into()))
                .collect(),
        );

        Ok(stream::once(res).boxed())
    }

    fn extract_in_band_error(response: &Self::Response) -> Option<anyhow::Error> {
        response
            .data
            .as_ref()
            .err()
            // A lost-ack replay is success-equivalent: the replica already holds
            // the chain and modern_sync advances its checkpoint. Do not count it
            // as a request error, or the metric hides real failures from on-call.
            .filter(|err| err.code != CODE_BOOKMARK_MOVE_ALREADY_PROCESSED)
            .map(|err| format_err!("{err:?}"))
    }
}

async fn replay_identical_moves_response<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    bookmark: String,
    moves: Vec<WireMirrorBookmarkMove>,
    pushvars: HashMap<String, Bytes>,
) -> anyhow::Result<ReplayIdenticalMovesResponse> {
    Ok(ReplayIdenticalMovesResponse {
        data: replay_identical_moves(repo, bookmark, moves, pushvars)
            .await
            .map_err(|e| {
                // A chain the replica already applied (a lost-ack replay) must
                // return a distinct code so modern_sync advances its checkpoint
                // instead of retrying the chain forever.
                if let Some(MononokeError::BookmarkMoveAlreadyProcessed) =
                    e.downcast_ref::<MononokeError>()
                {
                    ServerError::new(format!("{e:?}"), CODE_BOOKMARK_MOVE_ALREADY_PROCESSED)
                } else {
                    ServerError::generic(format!("{e:?}"))
                }
            }),
    })
}

async fn replay_identical_moves<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    bookmark: String,
    moves: Vec<WireMirrorBookmarkMove>,
    pushvars: HashMap<String, Bytes>,
) -> Result<(), Error> {
    let repo = repo.repo_ctx();

    let pushvars = if pushvars.is_empty() {
        None
    } else {
        Some(&pushvars)
    };

    // The wire moves already carry bonsai ids, so there is nothing to resolve.
    // modern_sync uploads each changeset to the replica under the source's own
    // bonsai id, so the source and the replica name a changeset the same way.
    // `replay_identical_moves` checks the whole chain against the commit graph
    // in one query.
    let moves = moves
        .into_iter()
        .map(|m| MirrorBookmarkMove {
            log_id: m.log_id,
            old: m.from.map(Into::into),
            new: m.to.into(),
            reason: mirror_reason(m.reason),
        })
        .collect();

    repo.replay_identical_moves(&BookmarkKey::new(&bookmark)?, moves, pushvars)
        .await?;
    Ok(())
}

/// Map the wire reason to the server reason. modern_sync copies each source
/// move's reason so the replica's log matches the source row for row.
fn mirror_reason(reason: MirrorBookmarkUpdateReason) -> BookmarkUpdateReason {
    match reason {
        MirrorBookmarkUpdateReason::Pushrebase => BookmarkUpdateReason::Pushrebase,
        MirrorBookmarkUpdateReason::Push => BookmarkUpdateReason::Push,
        MirrorBookmarkUpdateReason::Blobimport => BookmarkUpdateReason::Blobimport,
        MirrorBookmarkUpdateReason::ManualMove => BookmarkUpdateReason::ManualMove,
        MirrorBookmarkUpdateReason::TestMove => BookmarkUpdateReason::TestMove,
        MirrorBookmarkUpdateReason::Backsyncer => BookmarkUpdateReason::Backsyncer,
        MirrorBookmarkUpdateReason::XRepoSync => BookmarkUpdateReason::XRepoSync,
        MirrorBookmarkUpdateReason::ApiRequest => BookmarkUpdateReason::ApiRequest,
        MirrorBookmarkUpdateReason::MultiRepoLand => BookmarkUpdateReason::MultiRepoLand,
    }
}

/// Error wrapped bookmarks

#[async_trait]
impl SaplingRemoteApiHandler for Bookmarks2Handler {
    type Request = Bookmark2Request;
    type Response = BookmarkResult;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Bookmarks2;
    const ENDPOINT: &'static str = "/bookmarks2";
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] = &[
        SlapiCommitIdentityScheme::Hg,
        SlapiCommitIdentityScheme::Git,
    ];

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let slapi_flavour = ectx.slapi_flavour().clone();
        let repo = ectx.repo();
        let fetches = request.bookmarks.into_iter().map(move |bookmark| {
            let repo_ctx = repo.clone();
            async move {
                Ok(BookmarkResult {
                    data: fetch_bookmark(
                        repo_ctx,
                        bookmark,
                        slapi_flavour,
                        Freshness::from(request.freshness),
                    )
                    .await
                    .map_err(MononokeError::from)
                    .map_err(ServerError::from),
                })
            }
        });

        Ok(stream::iter(fetches)
            .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
            .boxed())
    }

    fn extract_in_band_error(response: &Self::Response) -> Option<Error> {
        response
            .data
            .as_ref()
            .err()
            .map(|err| format_err!("{err:?}"))
    }
}
