/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use edenapi_types::StreamingChangelogRequest;
use edenapi_types::StreamingChangelogResponse;
use mononoke_api::Repo;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;

/// Legacy streaming changelog handler from wireproto.
#[allow(dead_code)]
pub struct StreamingCloneHandler;

#[async_trait]
impl SaplingRemoteApiHandler for StreamingCloneHandler {
    type Request = StreamingChangelogRequest;
    type Response = StreamingChangelogResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Files2;
    const ENDPOINT: &'static str = "/streaming_clone";

    async fn handler(
        _ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        _request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        unimplemented!("StreamingCloneHandler is not implemented")
    }
}
