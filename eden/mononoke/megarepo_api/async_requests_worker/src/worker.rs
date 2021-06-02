/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! AsyncMethodRequestWorker is an object that provides everything that's needed
//! for processing the requests from the queue.
//!
//! In can grab requests from the queue, compute the result and update the
//! requests table with a response.

use crate::methods::megarepo_async_request_compute;
use async_requests::{
    types::MegarepoAsynchronousRequestParams, AsyncMethodRequestQueue, ClaimedBy, RequestId,
};
use async_stream::try_stream;
use context::CoreContext;
use futures::stream::{StreamExt, TryStreamExt};
use futures::Stream;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_types::RepositoryId;
use slog::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const DEQUEUE_STREAM_SLEEP_TIME: u64 = 1000;

#[derive(Clone)]
pub struct AsyncMethodRequestWorker {
    megarepo: MegarepoApi,
    name: String,
}

impl AsyncMethodRequestWorker {
    /// Creates a new tailer instance that's going to use provided megarepo API
    /// The name argument should uniquely identify tailer instance and will be put
    /// in the queue table so it's possible to find out which instance is working on
    /// a given task (for debugging purposes).
    pub fn new(megarepo: MegarepoApi, name: String) -> Self {
        Self { megarepo, name }
    }

    /// Start async request worker.
    /// If limit is set the worker will process a preset number of requests and
    /// return. If the limit is None the worker will be running continuously. The
    /// will_exit atomic bool is a flag to prevent the worker from grabbing new
    /// items from the queue and gracefully terminate.
    pub async fn run(
        &self,
        ctx: &CoreContext,
        will_exit: Arc<AtomicBool>,
        limit: Option<usize>,
        concurrency_limit: usize,
    ) -> Result<(), MegarepoError> {
        let queues_with_repos = self.megarepo.all_async_method_request_queues(ctx).await?;

        // Build stream that pools all the queues
        let request_stream = self
            .request_stream(ctx.clone(), queues_with_repos, will_exit)
            .boxed();

        let request_stream = if let Some(limit) = limit {
            request_stream.take(limit).left_stream()
        } else {
            request_stream.right_stream()
        };

        info!(
            ctx.logger(),
            "Worker initialization complete, starting request processing loop.",
        );


        request_stream
            .try_for_each_concurrent(Some(concurrency_limit), async move |(req_id, params)| {
                let worker = self.clone();
                let ctx = ctx.clone();
                let _updated = tokio::spawn(worker.compute_and_mark_completed(ctx, req_id, params))
                    .await
                    .map_err(MegarepoError::internal)??;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub fn request_stream(
        &self,
        ctx: CoreContext,
        queues_with_repos: Vec<(Vec<RepositoryId>, AsyncMethodRequestQueue)>,
        will_exit: Arc<AtomicBool>,
    ) -> impl Stream<Item = Result<(RequestId, MegarepoAsynchronousRequestParams), MegarepoError>>
    {
        let claimed_by = ClaimedBy(self.name.clone());
        try_stream! {
            let sleep_time = Duration::from_millis(DEQUEUE_STREAM_SLEEP_TIME);
            'outer: loop {
                let mut yielded = false;
                for (repo_ids, queue) in &queues_with_repos {
                    if will_exit.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                    if let Some((request_id, params)) = queue.dequeue(&ctx, &claimed_by, repo_ids).await? {
                        yield (request_id, params);
                        yielded = true;
                    }
                }
                if ! yielded {
                    // No requests in the queues, sleep before trying again.
                    debug!(
                        ctx.logger(),
                        "nothing to do, sleeping",
                    );
                    tokio::time::sleep(sleep_time).await;

                }
            }
        }
    }


    /// Params into stored response. Doesn't mark it as "in progress" (as this is done during dequeueing).
    /// Returns true if the result was successfully stored. Returns false if we
    /// lost the race (the request table was updated).
    async fn compute_and_mark_completed(
        self,
        ctx: CoreContext,
        req_id: RequestId,
        params: MegarepoAsynchronousRequestParams,
    ) -> Result<bool, MegarepoError> {
        let target = params.target()?.clone();

        info!(
            ctx.logger(),
            "[{}] new request:  id: {}, type: {}, repo_id: {}, bookmark: {}",
            &req_id.0,
            &req_id.0,
            &req_id.1,
            &target.repo_id,
            &target.bookmark,
        );

        // Do the actual work.
        let result = megarepo_async_request_compute(&ctx, &self.megarepo, params).await;
        info!(
            ctx.logger(),
            "[{}] request complete, saving result", &req_id.0
        );

        // Save the result.
        let updated = self
            .megarepo
            .async_method_request_queue(&ctx, &target)
            .await?
            .complete(&ctx, &req_id, result)
            .await?;

        info!(ctx.logger(), "[{}] result saved", &req_id.0);

        Ok(updated)
    }
}
