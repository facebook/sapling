/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_requests::types::Request;
use async_requests::types::ThriftParams;
use async_requests::types::Token;
use async_requests::AsyncMethodRequestQueue;
use context::CoreContext;
use mononoke_api::RepositoryId;

fn async_requests_disabled() -> scs_errors::ServiceError {
    scs_errors::internal_error(
        "Method is not supported when async requests are disabled".to_string(),
    )
    .into()
}

pub(crate) async fn enqueue<P: ThriftParams>(
    ctx: &CoreContext,
    queue: &Option<Arc<AsyncMethodRequestQueue>>,
    repo_id: Option<&RepositoryId>,
    params: P,
) -> Result<<<P::R as Request>::Token as Token>::ThriftToken, scs_errors::ServiceError> {
    match queue {
        Some(queue) => queue
            .enqueue(ctx, repo_id, params)
            .await
            .map(|res| res.into_thrift())
            .map_err(|e| {
                scs_errors::internal_error(format!("Failed to enqueue the request: {}", e)).into()
            }),
        None => Err(async_requests_disabled()),
    }
}

pub(crate) async fn poll<T: Token>(
    ctx: &CoreContext,
    queue: &Option<Arc<AsyncMethodRequestQueue>>,
    token: T,
) -> Result<<T::R as Request>::PollResponse, scs_errors::ServiceError> {
    match queue {
        Some(queue) => Ok(queue
            .poll(ctx, token)
            .await
            .map_err(scs_errors::poll_error)?),
        None => Err(async_requests_disabled()),
    }
}
