/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::Context;
use anyhow::anyhow;
use async_stream::try_stream;
use async_trait::async_trait;
use edenapi_types::SuffixQueryRequest;
use edenapi_types::SuffixQueryResponse;
use futures::StreamExt;
use gotham_ext::error::HttpError;
use itertools::EitherOrBoth;
use mononoke_api::ChangesetFileOrdering;
use mononoke_api::Repo;
use mononoke_types::MPath;
use types::RepoPathBuf;
use vec1::Vec1;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;
use crate::errors::SaplingRemoteApiServiceError;

pub struct SuffixQueryHandler;

#[async_trait]
impl SaplingRemoteApiHandler for SuffixQueryHandler {
    type Request = SuffixQueryRequest;
    type Response = SuffixQueryResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::SuffixQuery;
    const ENDPOINT: &'static str = "/suffix_query";

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero_ext::nonzero!(100u64)
    }

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let prefixes = request.prefixes.map(|prefixes| {
            prefixes
                .into_iter()
                .map(|prefix| MPath::try_from(&prefix).unwrap())
                .collect()
        });
        let suffixes = Vec1::try_from_vec(request.basename_suffixes)
            .with_context(|| anyhow!("No suffixes provided"))
            .map_err(HttpError::e400)?;
        let commit = request.commit.clone();

        // Changeset may return None if given an incorrect commit id.
        let changeset = repo
            .repo_ctx()
            .changeset(commit.clone())
            .await
            .with_context(|| anyhow!("Error getting changeset {}", commit.clone()))?
            .ok_or_else(|| SaplingRemoteApiServiceError::CommitIdNotFound(commit.clone()))
            .map_err(HttpError::e400)?;

        Ok(try_stream! {
            // Find files may return None if BSSM tree does not exist(eg. testing locally)
            // Will cause server to return 500 error.
            let matched_files = changeset
                .find_files_with_bssm_v3(
                    prefixes,
                    EitherOrBoth::Right(suffixes),
                    ChangesetFileOrdering::Unordered,
                ).await?;

            for await mpath in matched_files {
                let mpath = mpath?;
                let file_path = RepoPathBuf::from_string(mpath.to_string())?;
                yield SuffixQueryResponse {
                    file_path
                }
            }
        }
        .boxed())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use context::CoreContext;
    use edenapi_types::CommitId;
    use fbinit::FacebookInit;
    use futures::FutureExt;
    use gotham::extractor::NoopQueryStringExtractor;
    use gotham_ext::handler::SlapiCommitIdentityScheme;
    use gotham_ext::middleware::request_context::RequestContext;
    use mononoke_api::Mononoke;
    use mononoke_api_hg::RepoContextHgExt;
    use mononoke_macros::mononoke;
    use test_repo_factory::TestRepoFactory;
    use types::HgId;

    use super::*;
    use crate::context::ServerContext;
    use crate::handlers::handler::BasicPathExtractor;

    type TestContext = SaplingRemoteApiContext<BasicPathExtractor, NoopQueryStringExtractor, Repo>;

    async fn suffix_query_context(fb: FacebookInit) -> anyhow::Result<TestContext> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = TestRepoFactory::new(fb)?.build().await?;
        let mononoke = Arc::new(Mononoke::new_test(vec![("test".to_string(), repo)]).await?);
        let hg_repo = mononoke
            .repo(ctx.clone(), "test")
            .await?
            .expect("test repo is loaded")
            .build()
            .await?
            .hg();
        let sctx = ServerContext::new(mononoke, Arc::new(AtomicBool::new(false)));
        Ok(SaplingRemoteApiContext::new(
            RequestContext { ctx },
            sctx,
            hg_repo,
            BasicPathExtractor {
                repo: vec!["test".to_string()],
            },
            NoopQueryStringExtractor,
            SlapiCommitIdentityScheme::Hg,
        ))
    }

    /// FIXME: BUG! This did panic inside `SuffixQueryHandler::handler` when a client-supplied
    /// `prefixes` entry was not a valid `MPath` (here `..`; also `.`, a component with a NUL
    /// byte or a component longer than 255 bytes) but should have rejected the request with
    /// HTTP 400 like every other handler that takes paths. The panic is not caught by
    /// gotham_ext, so it tears down the whole HTTP connection with every stream multiplexed
    /// on it. This test pins the current behaviour; the next diff fixes it.
    #[mononoke::fbinit_test]
    async fn test_suffix_query_invalid_prefix(fb: FacebookInit) -> anyhow::Result<()> {
        let ectx = suffix_query_context(fb).await?;
        let request = SuffixQueryRequest {
            commit: CommitId::Hg(HgId::null_id().clone()),
            basename_suffixes: vec![".rs".to_string()],
            prefixes: Some(vec!["..".to_string()]),
        };

        let outcome = std::panic::AssertUnwindSafe(SuffixQueryHandler::handler(ectx, request))
            .catch_unwind()
            .await;

        // FIXME: BUG! The handler panics on client input instead of returning a 400 error.
        assert!(outcome.is_err());
        Ok(())
    }
}
