/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::request_context::RequestContext;
use http::HeaderMap;
use http_body_util::BodyExt as _;
use mononoke_api::MononokeRepo;
use mononoke_api_hg::HgRepoContext;
use mononoke_api_hg::RepoContextHgExt;
use rate_limiting::Metric;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::errors::MononokeErrorExt;
use crate::middleware::request_dumper::RequestDumper;

pub mod cbor;
pub mod commit_cloud_types;
pub mod convert;
pub mod monitor;
pub mod rate_limit;
#[cfg(fbcode_build)]
pub mod rim_shadow;

pub use cbor::cbor_mime;
pub use cbor::cbor_stream_filtered_errors;
pub use cbor::custom_cbor_stream;
pub use cbor::parse_cbor_request;
pub use cbor::parse_wire_request;
pub use cbor::to_cbor_bytes;
pub use convert::to_create_change;
pub use convert::to_hg_path;
pub use convert::to_hg_path_nonroot;
pub use convert::to_mpath;
pub use convert::to_revlog_changeset;
pub use rate_limit::build_counter;
pub use rate_limit::counter_check_and_bump;

pub async fn get_repo<R: MononokeRepo>(
    sctx: &ServerContext<R>,
    rctx: &RequestContext,
    name: impl AsRef<str>,
    throttle_metric: impl Into<Option<Metric>>,
) -> Result<HgRepoContext<R>, HttpError> {
    let mut scuba = rctx.ctx.scuba().clone();
    rctx.ctx.session().check_load_shed(&mut scuba)?;

    if let Some(throttle_metric) = throttle_metric.into() {
        rctx.ctx
            .session()
            .check_rate_limit(throttle_metric, &mut scuba)
            .await?;
    }

    let name = name.as_ref();
    let repo = sctx
        .mononoke_api()
        .repo(rctx.ctx.with_mutated_scuba(|_| scuba), name)
        .await
        .map_err(|e| e.into_http_error(ErrorKind::RepoLoadFailed(name.to_string())))?;

    let repo = match repo {
        Some(repo) => repo,
        None => {
            // The repo is not loaded on this task. In a sharded deployment it may
            // still exist tier-wide but be assigned to a different shard, so return
            // a retriable 503 in that case and reserve 404 for repos that are truly
            // unknown to the tier.
            return if sctx
                .mononoke_api()
                .repo_names_in_tier
                .load()
                .contains_key(name)
            {
                Err(HttpError::e503(ErrorKind::RepoNotLoaded(name.to_string())))
            } else {
                Err(HttpError::e404(ErrorKind::RepoDoesNotExist(
                    name.to_string(),
                )))
            };
        }
    };

    repo.build()
        .await
        .map(|repo| repo.hg())
        .map_err(|e| e.into_http_error(ErrorKind::RepoLoadFailed(name.to_string())))
}

pub async fn get_request_body(state: &mut State) -> Result<Bytes, HttpError> {
    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);
    let body = body
        .into_data_stream()
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
