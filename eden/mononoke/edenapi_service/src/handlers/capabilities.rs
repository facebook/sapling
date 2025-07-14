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
use gotham_ext::error::HttpError;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::response::BytesBody;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;
use serde::Deserialize;

use crate::context::ServerContext;
use crate::errors::MononokeErrorExt;
use crate::handlers::HandlerInfo;
use crate::handlers::SaplingRemoteApiMethod;
use crate::utils::get_repo;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct CapabilitiesParams {
    repo: String,
}

/// Features frequently used by sapling operations like commit upload, megarepo, blame, etc
static CAP_SAPLING_COMMON: &str = "sapling-common";
/// These determine whether the server supports commit graph for clones
static CAP_COMMIT_GRAPH_SEGMENTS: &str = "commit-graph-segments";
/// This indicates the commit cloud family of endpoints are available
static CAP_COMMIT_CLOUD: &str = "commit-cloud";

/// Get capabilities as a vector of static strings.
///
/// Capabilities are used for optional features (i.e. it's valid to have the
/// feature on, and also valid to have the feature off, for a long time).
/// Features that are designed to be always on might not qualify as
/// capabilities.
async fn get_capabilities_vec<R>(
    _hg_repo_ctx: &HgRepoContext<R>,
) -> Result<Vec<&'static str>, MononokeError> {
    let mut capabilities = Vec::new();

    capabilities.push(CAP_SAPLING_COMMON);
    capabilities.push(CAP_COMMIT_GRAPH_SEGMENTS);
    capabilities.push(CAP_COMMIT_CLOUD);

    Ok(capabilities)
}

pub async fn capabilities_handler(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = CapabilitiesParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        SaplingRemoteApiMethod::Capabilities,
    ));

    let sctx = ServerContext::borrow_from(state);
    let rctx = RequestContext::borrow_from(state).clone();
    let hg_repo_ctx: HgRepoContext<Repo> = get_repo(sctx, &rctx, &params.repo, None).await?;

    let slapi_flavour = SlapiCommitIdentityScheme::borrow_from(state).clone();

    let caps = match slapi_flavour {
        SlapiCommitIdentityScheme::Hg => get_capabilities_vec(&hg_repo_ctx)
            .await
            .map_err(|e| e.into_http_error("error getting capabilities"))?,
        SlapiCommitIdentityScheme::Git => {
            vec![]
        }
    };

    let caps_json = serde_json::to_vec(&caps).map_err(|e| {
        HttpError::e500(anyhow::Error::from(e).context("converting capabilities to JSON"))
    })?;
    Ok(BytesBody::new(caps_json.into(), mime::APPLICATION_JSON))
}
