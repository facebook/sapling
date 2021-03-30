/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::{stream, Stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use serde::Deserialize;

use bookmarks::Freshness;
use mononoke_api_hg::HgRepoContext;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_wire_request};

use super::{EdenApiMethod, HandlerInfo};

use edenapi_types::{
    wire::{ToWire, WireBookmarkRequest},
    BookmarkEntry, BookmarkRequest, HgId,
};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 100;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct BookmarksParams {
    repo: String,
}

/// Resolve the bookmarks requested by the client.
pub async fn bookmarks(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = BookmarksParams::take_from(state);
    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Bookmarks));
    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;

    let request = parse_wire_request::<WireBookmarkRequest>(state).await?;
    Ok(cbor_stream(
        rctx,
        fetch_all_bookmarks(repo, request).map(|r| r.map(|v| v.to_wire())),
    ))
}

/// Fetch the value of all the requested bookmarks concurrently.
fn fetch_all_bookmarks(
    repo: HgRepoContext,
    request: BookmarkRequest,
) -> impl Stream<Item = Result<BookmarkEntry, Error>> {
    let fetches = request
        .bookmarks
        .into_iter()
        .map(move |bookmark| fetch_bookmark(repo.clone(), bookmark));

    stream::iter(fetches).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
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
