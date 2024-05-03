/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use async_trait::async_trait;
use edenapi_types::SuffixQueryRequest;
use edenapi_types::SuffixQueryResponse;
use futures::stream;
use futures::StreamExt;
use types::RepoPathBuf;

use super::handler::EdenApiContext;
use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerResult;

pub struct SuffixQueryHandler;

#[async_trait]
impl EdenApiHandler for SuffixQueryHandler {
    type Request = SuffixQueryRequest;
    type Response = SuffixQueryResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::SuffixQuery;
    const ENDPOINT: &'static str = "/suffix_query";

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero_ext::nonzero!(100u64)
    }

    async fn handler(
        _ectx: EdenApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        _request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        // Stub function
        let result = vec![
            Ok(SuffixQueryResponse {
                file_path: RepoPathBuf::new(),
            }),
            Ok(SuffixQueryResponse {
                file_path: RepoPathBuf::new(),
            }),
        ];
        Ok(stream::iter(result).boxed())
    }
}
