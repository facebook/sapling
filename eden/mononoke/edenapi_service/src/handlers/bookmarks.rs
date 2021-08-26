/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bookmarks::Freshness;
use edenapi_types::{BookmarkEntry, BookmarkRequest, HgId};
use futures::{stream, StreamExt};
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
