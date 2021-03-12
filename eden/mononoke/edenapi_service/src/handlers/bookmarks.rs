/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;

use bookmarks::Freshness;
use bytes::Bytes;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use gotham_ext::{error::HttpError, response::BytesBody};
use mercurial_types::HgChangesetId;
use serde::{Deserialize, Serialize};

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::errors::MononokeErrorExt;
use crate::middleware::RequestContext;
use crate::utils::get_repo;

use super::{EdenApiMethod, HandlerInfo};

/// TODO: add Edenapi and Edenapi::wire type for request and response type
///       add support for prefix listing
#[derive(Clone, Serialize, Debug)]
struct BookmarksResponse {
    bookmark_value: Option<HgChangesetId>,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct BookmarksParams {
    repo: String,
    bookmark: String,
}

pub async fn bookmarks(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = BookmarksParams::take_from(state);
    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Bookmarks));
    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;
    let bookmark_value = repo
        .resolve_bookmark(params.bookmark, Freshness::MaybeStale)
        .await
        .map_err(|e| e.into_http_error("error resolving bookmark"))?;

    // TODO: add cbor serialization when the response type is changed to an
    // Edenapi wire type.
    let bytes: Bytes = serde_json::to_string(&BookmarksResponse { bookmark_value })
        .context(ErrorKind::SerializationFailed)
        .map_err(HttpError::e500)?
        .into();
    Ok(BytesBody::new(bytes, mime::APPLICATION_JSON))
}
