/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use async_trait::async_trait;
use edenapi_types::ToWire;
use futures::stream::BoxStream;
use gotham::extractor::PathExtractor;
use gotham::extractor::QueryStringExtractor;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::request_context::RequestContext;
use hyper::body::Body;
use mononoke_api::MononokeError;
use mononoke_api_hg::HgRepoContext;
use nonzero_ext::nonzero;
use serde::Deserialize;

use super::EdenApiMethod;
use crate::context::ServerContext;
use crate::utils::get_repo;

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

pub struct HandlerError(HttpError);

impl From<HandlerError> for HttpError {
    fn from(e: HandlerError) -> Self {
        e.0
    }
}

// Default errors to 500.
impl From<MononokeError> for HandlerError {
    fn from(e: MononokeError) -> Self {
        Self(HttpError::e500(e))
    }
}

impl From<anyhow::Error> for HandlerError {
    fn from(e: anyhow::Error) -> Self {
        Self(HttpError::e500(e))
    }
}

// Handlers can propagate HttpError for a specific response code.
impl From<HttpError> for HandlerError {
    fn from(e: HttpError) -> Self {
        Self(e)
    }
}

pub type HandlerResult<'a, Response> =
    Result<BoxStream<'a, anyhow::Result<Response>>, HandlerError>;

pub struct EdenApiContext<P, Q> {
    rctx: RequestContext,
    sctx: ServerContext,
    repo: HgRepoContext,
    path: P,
    query: Q,
}

impl<P, Q> EdenApiContext<P, Q> {
    pub fn new(
        rctx: RequestContext,
        sctx: ServerContext,
        repo: HgRepoContext,
        path: P,
        query: Q,
    ) -> Self {
        Self {
            rctx,
            sctx,
            repo,
            path,
            query,
        }
    }
    pub fn repo(&self) -> HgRepoContext {
        self.repo.clone()
    }

    #[allow(unused)]
    pub fn path(&self) -> &P {
        &self.path
    }

    pub fn query(&self) -> &Q {
        &self.query
    }

    /// Open an "other" repo (i.e. distinct from repo specified in URL path).
    pub async fn other_repo(&self, repo_name: impl AsRef<str>) -> Result<HgRepoContext, HttpError> {
        get_repo(&self.sctx, &self.rctx, repo_name, None).await
    }
}

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
        ctx: EdenApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response>;
}
