/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::EdenApiMethod;
use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::ToWire;
use futures::stream::BoxStream;
use gotham::extractor::{PathExtractor, QueryStringExtractor};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::body::Body;
use mononoke_api_hg::HgRepoContext;
use serde::Deserialize;

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

    async fn handler(
        repo: HgRepoContext,
        path: Self::PathExtractor,
        query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> Result<BoxStream<'async_trait, Result<Self::Response>>>;
}
