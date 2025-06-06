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
use gotham_ext::handler::SlapiCommitIdentityScheme;
use gotham_ext::middleware::request_context::RequestContext;
use hyper::body::Body;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;
use nonzero_ext::nonzero;
use serde::Deserialize;

use super::SaplingRemoteApiMethod;
use crate::context::ServerContext;
use crate::utils::get_repo;

pub trait PathExtractorWithRepo: PathExtractor<Body> + Send + Sync {
    fn repo(&self) -> String;
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct BasicPathExtractor {
    /// The name of the repository. It is a vec of strings because repo with `/` in their
    /// names are captured as multiple segments in the path.
    pub repo: Vec<String>,
}

impl PathExtractorWithRepo for BasicPathExtractor {
    fn repo(&self) -> String {
        let repo = self.repo.join("/");
        match repo.strip_suffix(".git") {
            Some(repo) => repo.to_string(),
            None => repo,
        }
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

pub struct SaplingRemoteApiContext<P, Q, R: Send + Sync + 'static> {
    rctx: RequestContext,
    sctx: ServerContext<R>,
    repo: HgRepoContext<R>,
    path: P,
    query: Q,
    slapi_flavour: SlapiCommitIdentityScheme,
}

impl<P, Q, R: Send + Sync + 'static> SaplingRemoteApiContext<P, Q, R> {
    pub fn new(
        rctx: RequestContext,
        sctx: ServerContext<R>,
        repo: HgRepoContext<R>,
        path: P,
        query: Q,
        slapi_flavour: SlapiCommitIdentityScheme,
    ) -> Self {
        Self {
            rctx,
            sctx,
            repo,
            path,
            query,
            slapi_flavour,
        }
    }
    pub fn repo(&self) -> HgRepoContext<R>
    where
        R: Clone,
    {
        self.repo.clone()
    }

    #[allow(unused)]
    pub fn slapi_flavour(&self) -> SlapiCommitIdentityScheme {
        self.slapi_flavour
    }

    #[allow(unused)]
    pub fn path(&self) -> &P {
        &self.path
    }

    pub fn query(&self) -> &Q {
        &self.query
    }

    /// Open an "other" repo (i.e. distinct from repo specified in URL path).
    pub async fn other_repo(
        &self,
        repo_name: impl AsRef<str>,
    ) -> Result<HgRepoContext<R>, HttpError>
    where
        R: MononokeRepo,
    {
        get_repo(&self.sctx, &self.rctx, repo_name, None).await
    }
}

#[async_trait]
pub trait SaplingRemoteApiHandler: 'static {
    type PathExtractor: PathExtractorWithRepo = BasicPathExtractor;
    type QueryStringExtractor: QueryStringExtractor<Body> + Send + Sync =
        gotham::extractor::NoopQueryStringExtractor;
    type Request: ToWire + Send;
    type Response: ToWire + Send + 'static;

    const HTTP_METHOD: hyper::Method;
    const API_METHOD: SaplingRemoteApiMethod;
    /// DON'T include the /:repo prefix.
    /// Example: "/ephemeral/prepare"
    const ENDPOINT: &'static str;

    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] =
        &[SlapiCommitIdentityScheme::Hg];

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero!(1u64)
    }

    async fn handler(
        ctx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response>;
}
