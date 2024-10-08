/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use mononoke_api::Repo;

use super::handler::HandlerError;
use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
struct GitObjectsHandler;

#[async_trait]
impl SaplingRemoteApiHandler for GitObjectsHandler {
    type Request = ();
    type Response = ();

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::GitObjects;
    const ENDPOINT: &'static str = "/git_objects";

    async fn handler(
        _ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        _request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        Err(HandlerError::from(anyhow::anyhow!("Not implemented")))
    }
}
