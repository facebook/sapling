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
use client::AsyncRequestsQueue;
use context::CoreContext;
use mononoke_api::RepositoryId;

use crate::errors;

pub(crate) async fn get_queue(
    ctx: &CoreContext,
    async_requests_queue_client: &Option<Arc<AsyncRequestsQueue>>,
) -> Result<AsyncMethodRequestQueue, errors::ServiceError> {
    match async_requests_queue_client {
        Some(queue_client) => Ok(queue_client.async_method_request_queue(ctx).await?),
        None => Err(async_requests_disabled()),
    }
}

fn async_requests_disabled() -> errors::ServiceError {
    errors::internal_error("Method is not supported when async requests are disabled".to_string())
        .into()
}

pub(crate) async fn enqueue<P: ThriftParams>(
    ctx: &CoreContext,
    queue: &AsyncMethodRequestQueue,
    repo_id: Option<&RepositoryId>,
    params: P,
) -> Result<<<P::R as Request>::Token as Token>::ThriftToken, errors::ServiceError> {
    queue
        .enqueue(ctx, repo_id, params)
        .await
        .map(|res| res.into_thrift())
        .map_err(|e| errors::internal_error(format!("Failed to enqueue the request: {}", e)).into())
}

pub(crate) async fn poll<T: Token>(
    ctx: &CoreContext,
    queue: &AsyncMethodRequestQueue,
    token: T,
) -> Result<<T::R as Request>::PollResponse, errors::ServiceError> {
    Ok(queue
        .poll(ctx, token)
        .await
        .map_err(errors::internal_error)?)
}
