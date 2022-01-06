/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use gotham::state::{FromState, State};
use http::HeaderMap;
use hyper::Body;

use gotham_ext::{body_ext::BodyExt, error::HttpError};
use mononoke_api_hg::{HgRepoContext, RepoContextHgExt};
use rate_limiting::Metric;

use crate::context::ServerContext;
use crate::errors::{ErrorKind, MononokeErrorExt};
use crate::middleware::RequestContext;

pub mod cbor;
pub mod convert;

pub use cbor::{
    cbor_mime, cbor_stream_filtered_errors, custom_cbor_stream, parse_cbor_request,
    parse_wire_request, to_cbor_bytes,
};
pub use convert::{to_create_change, to_hg_path, to_mononoke_path, to_mpath, to_revlog_changeset};

pub async fn get_repo(
    sctx: &ServerContext,
    rctx: &RequestContext,
    name: impl AsRef<str>,
    throttle_metric: impl Into<Option<Metric>>,
) -> Result<HgRepoContext, HttpError> {
    rctx.ctx.session().check_load_shed()?;

    if let Some(throttle_metric) = throttle_metric.into() {
        rctx.ctx.session().check_rate_limit(throttle_metric).await?;
    }

    let name = name.as_ref();
    sctx.mononoke_api()
        .repo(rctx.ctx.clone(), name)
        .await
        .map_err(|e| e.into_http_error(ErrorKind::RepoLoadFailed(name.to_string())))?
        .map(|repo| repo.hg())
        .with_context(|| ErrorKind::RepoDoesNotExist(name.to_string()))
        .map_err(HttpError::e404)
}

pub async fn get_request_body(state: &mut State) -> Result<Bytes, HttpError> {
    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);
    body.try_concat_body_opt(headers)
        .context(ErrorKind::InvalidContentLength)
        .map_err(HttpError::e400)?
        .await
        .context(ErrorKind::ClientCancelled)
        .map_err(HttpError::e400)
}
