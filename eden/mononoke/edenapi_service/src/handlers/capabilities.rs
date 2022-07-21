/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use serde::Deserialize;

use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use mononoke_api::MononokeError;

use crate::context::ServerContext;
use crate::errors::MononokeErrorExt;
use crate::handlers::EdenApiMethod;
use crate::handlers::HandlerInfo;
use crate::middleware::RequestContext;
use crate::utils::get_repo;
use mononoke_api_hg::HgRepoContext;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct CapabilitiesParams {
    repo: String,
}

static CAP_SEGMENTED_CHANGELOG: &str = "segmented-changelog";

/// Get capabilities as a vector of static strings.
///
/// Capabilities are used for optional features (i.e. it's valid to have the
/// feature on, and also valid to have the feature off, for a long time).
/// Features that are designed to be always on might not qualify as
/// capabilities.
async fn get_capabilities_vec(
    hg_repo_ctx: &HgRepoContext,
) -> Result<Vec<&'static str>, MononokeError> {
    let mut capabilities = Vec::new();

    if !hg_repo_ctx.segmented_changelog_disabled().await? {
        capabilities.push(CAP_SEGMENTED_CHANGELOG);
    }

    Ok(capabilities)
}

pub async fn capabilities_handler(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = CapabilitiesParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Capabilities));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx = get_repo(sctx, &rctx, &params.repo, None).await?;
    let caps = get_capabilities_vec(&hg_repo_ctx)
        .await
        .map_err(|e| e.into_http_error("error getting capabilities"))?;
    let caps_json = serde_json::to_vec(&caps).map_err(|e| {
        HttpError::e500(anyhow::Error::from(e).context("converting capabilities to JSON"))
    })?;
    Ok(BytesBody::new(caps_json.into(), mime::APPLICATION_JSON))
}
