/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::EdenApiMethod;
use async_trait::async_trait;
use edenapi_types::ToWire;
use futures::stream::BoxStream;
use gotham::extractor::PathExtractor;
use gotham::extractor::QueryStringExtractor;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use hyper::body::Body;
use mononoke_api_hg::HgRepoContext;
use nonzero_ext::nonzero;
use serde::Deserialize;
use std::num::NonZeroU64;

pub trait PathExtractorWithRepo: PathExtractor<Body> + Send + Sync {
    fn repo(&self) -> &str;
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct BasicPathExtractor {
    repo: String,
}

impl PathExtractorWithRepo for BasicPathExtractor {
    fn repo(&self) -> &str {
        &self.repo
    }
}

pub enum HandlerError {
    E500(anyhow::Error),
}

// Default errors to 500
impl<E> From<E> for HandlerError
where
    E: Into<anyhow::Error>,
{
    fn from(e: E) -> Self {
        Self::E500(e.into())
    }
}

pub type HandlerResult<'a, Response> =
    Result<BoxStream<'a, anyhow::Result<Response>>, HandlerError>;

#[async_trait]
pub trait EdenApiHandler: 'static {
    type PathExtractor: PathExtractorWithRepo = BasicPathExtractor;
    type QueryStringExtractor: QueryStringExtractor<Body> + Send + Sync =
        gotham::extractor::NoopQueryStringExtractor;
    type Request: ToWire + Send;
    type Response: ToWire + Send + 'static;

    const HTTP_METHOD: hyper::Method;
    const API_METHOD: EdenApiMethod;
    /// DON'T include the /:repo prefix.
    /// Example: "/ephemeral/prepare"
    const ENDPOINT: &'static str;

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero!(1u64)
    }

    async fn handler(
        repo: HgRepoContext,
        path: Self::PathExtractor,
        query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response>;
}
