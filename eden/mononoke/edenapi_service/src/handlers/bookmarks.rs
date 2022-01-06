/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{Context, Error};
use async_trait::async_trait;
use bookmarks::Freshness;
use bytes::Bytes;
use edenapi_types::{BookmarkEntry, BookmarkRequest, HgId, SetBookmarkRequest};
use futures::{stream, StreamExt};
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_api_hg::HgRepoContext;

use crate::errors::ErrorKind;

use super::{EdenApiHandler, EdenApiMethod, HandlerResult};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;

/// Resolve the bookmarks requested by the client.
pub struct BookmarksHandler;

#[async_trait]
impl EdenApiHandler for BookmarksHandler {
    type Request = BookmarkRequest;
    type Response = BookmarkEntry;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::Bookmarks;
    const ENDPOINT: &'static str = "/bookmarks";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let fetches = request
            .bookmarks
            .into_iter()
            .map(move |bookmark| fetch_bookmark(repo.clone(), bookmark));

        Ok(stream::iter(fetches)
            .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
            .boxed())
    }
}

/// Fetch the value of a single bookmark.
async fn fetch_bookmark(repo: HgRepoContext, bookmark: String) -> Result<BookmarkEntry, Error> {
    let hgid = repo
        .resolve_bookmark(bookmark.clone(), Freshness::MaybeStale)
        .await
        .map_err(|_| ErrorKind::BookmarkResolutionFailed(bookmark.clone()))?
        .map(|id| HgId::from(id.into_nodehash()));
    Ok(BookmarkEntry { bookmark, hgid })
}

/// Create, delete, or move a bookmark
pub struct SetBookmarkHandler;

#[async_trait]
impl EdenApiHandler for SetBookmarkHandler {
    type Request = SetBookmarkRequest;
    type Response = ();

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::SetBookmark;
    const ENDPOINT: &'static str = "/bookmarks/set";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        Ok(stream::once(set_bookmark(
            repo.clone(),
            request.bookmark,
            request.to,
            request.from,
            request
                .pushvars
                .into_iter()
                .map(|p| (p.key, p.value.into()))
                .collect(),
        ))
        .boxed())
    }
}

async fn set_bookmark(
    repo: HgRepoContext,
    bookmark: String,
    to: Option<HgId>,
    from: Option<HgId>,
    pushvars: HashMap<String, Bytes>,
) -> Result<(), Error> {
    let repo = repo.write().await?;

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
                .ok_or_else(|| ErrorKind::HgIdNotFound(to_hgid))?
                .id();

            let from = HgChangesetId::new(HgNodeHash::from(from_hgid));
            let from = repo
                .changeset(from)
                .await
                .context("failed to resolve 'from' hgid")?
                .ok_or_else(|| ErrorKind::HgIdNotFound(from_hgid))?
                .id();

            repo.move_bookmark(bookmark, to, Some(from), true, pushvars)
                .await?
        }
        (Some(to_hgid), None) => {
            // Create bookmark
            let to = HgChangesetId::new(HgNodeHash::from(to_hgid));
            let to = repo
                .changeset(to)
                .await
                .context("failed to resolve 'to' hgid")?
                .ok_or_else(|| ErrorKind::HgIdNotFound(to_hgid))?
                .id();

            repo.create_bookmark(bookmark, to, pushvars).await?
        }
        (None, Some(from_hgid)) => {
            // Delete bookmark
            let from = HgChangesetId::new(HgNodeHash::from(from_hgid));
            let from = repo
                .changeset(from)
                .await
                .context("failed to resolve 'from' hgid")?
                .ok_or_else(|| ErrorKind::HgIdNotFound(from_hgid))?
                .id();

            repo.delete_bookmark(bookmark, Some(from), pushvars).await?
        }
        (None, None) => {
            return Err(Error::msg(
                "invalid SetBookmarkRequest, must specify at least one of 'to' or 'from'",
            ));
        }
    })
}
