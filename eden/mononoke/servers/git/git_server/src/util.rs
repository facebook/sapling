/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use bytes::Bytes;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfigRef;
use git_source_of_truth::RepositoryName;
use git_source_of_truth::Staleness;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use gotham_ext::response::EmptyBody;
use gotham_ext::response::TryIntoResponse;
use http::HeaderMap;
use http::Response;
use hyper::Body;
use mononoke_api::CoreContext;
use mononoke_api::Repo;
use repo_identity::RepoIdentityRef;

pub async fn get_body(state: &mut State) -> Result<Bytes, HttpError> {
    Body::take_from(state)
        .try_concat_body(&HeaderMap::new())
        .map_err(HttpError::e500)?
        .await
        .map_err(HttpError::e500)
}

pub fn empty_body(state: &mut State) -> Result<Response<Body>, HttpError> {
    EmptyBody::new()
        .try_into_response(state)
        .map_err(HttpError::e500)
}

pub async fn mononoke_source_of_truth(ctx: &CoreContext, repo: Arc<Repo>) -> anyhow::Result<bool> {
    let repo_name = RepositoryName(repo.repo_identity().name().to_string());
    repo.git_source_of_truth_config()
        .get_by_repo_name(ctx, &repo_name, Staleness::MostRecent)
        .await
        .map(|entry| entry.is_some_and(|entry| entry.source_of_truth == GitSourceOfTruth::Mononoke))
}
