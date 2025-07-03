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
use bookmarks::Freshness;
use bytes::Bytes;
use edenapi_types::BookmarkEntry;
use edenapi_types::BookmarkRequest;
use edenapi_types::BookmarkResult;
use edenapi_types::HgId;
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

/// Resolve the bookmarks requested by the client.
pub struct BookmarksHandler;
pub struct Bookmarks2Handler;

#[async_trait]
impl SaplingRemoteApiHandler for BookmarksHandler {
    type Request = BookmarkRequest;
    type Response = BookmarkEntry;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Bookmarks;
    const ENDPOINT: &'static str = "/bookmarks";
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
            fetch_bookmark(repo.clone(), bookmark, slapi_flavour, Freshness::MaybeStale)
        });

        Ok(stream::iter(fetches)
            .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
            .boxed())
    }
}

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
            .map_err(|_| ErrorKind::BookmarkResolutionFailed(bookmark.clone()))?
            .map(|id| HgId::from_slice(id.as_ref()))
            .transpose()?,
        SlapiCommitIdentityScheme::Hg => repo
            .resolve_bookmark(bookmark.clone(), freshness)
            .await
            .map_err(|_| ErrorKind::BookmarkResolutionFailed(bookmark.clone()))?
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

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
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
            .map(|err| format_err!("{:?}", err))
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
            .map_err(|e| ServerError::generic(format!("{:?}", e))),
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
            let to = repo
                .changeset(to)
                .await
                .context("failed to resolve 'to' hgid")?
                .ok_or(ErrorKind::HgIdNotFound(to_hgid))?
                .id();

            let from = HgChangesetId::new(HgNodeHash::from(from_hgid));
            let from = repo
                .changeset(from)
                .await
                .context("failed to resolve 'from' hgid")?
                .ok_or(ErrorKind::HgIdNotFound(from_hgid))?
                .id();

            repo.move_bookmark(
                &BookmarkKey::new(&bookmark)?,
                to,
                Some(from),
                true,
                pushvars,
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

            repo.create_bookmark(&BookmarkKey::new(&bookmark)?, to, pushvars)
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

/// Error wrapped bookmarks

#[async_trait]
impl SaplingRemoteApiHandler for Bookmarks2Handler {
    type Request = Bookmark2Request;
    type Response = BookmarkResult;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
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
            .map(|err| format_err!("{:?}", err))
    }
}
