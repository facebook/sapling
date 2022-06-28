/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use http::HeaderMap;
use hyper::Body;

use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use mononoke_api_hg::HgRepoContext;
use mononoke_api_hg::RepoContextHgExt;
use rate_limiting::Metric;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::errors::MononokeErrorExt;
use crate::middleware::request_dumper::RequestDumper;
use crate::middleware::RequestContext;

pub mod cbor;
pub mod convert;

pub use cbor::cbor_mime;
pub use cbor::cbor_stream_filtered_errors;
pub use cbor::custom_cbor_stream;
pub use cbor::parse_cbor_request;
pub use cbor::parse_wire_request;
pub use cbor::to_cbor_bytes;
pub use convert::to_create_change;
pub use convert::to_hg_path;
pub use convert::to_mononoke_path;
pub use convert::to_mpath;
pub use convert::to_revlog_changeset;

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
        .with_context(|| ErrorKind::RepoDoesNotExist(name.to_string()))
        .map_err(HttpError::e404)?
        .build()
        .await
        .map(|repo| repo.hg())
        .map_err(|e| e.into_http_error(ErrorKind::RepoLoadFailed(name.to_string())))
}

pub async fn get_request_body(state: &mut State) -> Result<Bytes, HttpError> {
    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);
    let body = body
        .try_concat_body_opt(headers)
        .context(ErrorKind::InvalidContentLength)
        .map_err(HttpError::e400)?
        .await
        .context(ErrorKind::ClientCancelled)
        .map_err(HttpError::e400)?;

    if let Some(rd) = RequestDumper::try_borrow_mut_from(state) {
        rd.add_body(&body);
    };

    Ok(body)
}
