/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! AsyncMethodRequestWorker is an object that provides everything that's needed
//! for processing the requests from the queue.
//!
//! In can grab requests from the queue, compute the result and update the
//! requests table with a response.
//! One important consideration to keep in mind - worker executes request "at least once"
//! but not exactly once i.e. the same request might be executed a few times.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use async_requests::types::IntoConfigFormat;
use async_requests::types::MegarepoAsynchronousRequestParams;
use async_requests::AsyncMethodRequestQueue;
use async_requests::ClaimedBy;
use async_requests::RequestId;
use async_stream::try_stream;
use cloned::cloned;
use context::CoreContext;
use futures::future::abortable;
use futures::future::select;
use futures::future::Either;
use futures::pin_mut;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use megarepo_api::MegarepoApi;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use slog::debug;
use slog::error;
use slog::info;

use crate::methods::megarepo_async_request_compute;

const DEQUEUE_STREAM_SLEEP_TIME: u64 = 1000;
// Number of seconds after which inprogress request is considered abandoned
// if it hasn't updated inprogress timestamp
const ABANDONED_REQUEST_THRESHOLD_SECS: i64 = 5 * 60;
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct AsyncMethodRequestWorker {
    megarepo: Arc<MegarepoApi>,
    name: String,
}

impl AsyncMethodRequestWorker {
    /// Creates a new tailer instance that's going to use provided megarepo API
    /// The name argument should uniquely identify tailer instance and will be put
    /// in the queue table so it's possible to find out which instance is working on
    /// a given task (for debugging purposes).
    pub fn new(megarepo: Arc<MegarepoApi>, name: String) -> Self {
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
        let sleep_time = Duration::from_millis(DEQUEUE_STREAM_SLEEP_TIME);
        Self::request_stream_inner(
            ctx,
            claimed_by,
            queues_with_repos,
            will_exit,
            sleep_time,
            ABANDONED_REQUEST_THRESHOLD_SECS,
        )
    }

    fn request_stream_inner(
        ctx: CoreContext,
        claimed_by: ClaimedBy,
        queues_with_repos: Vec<(Vec<RepositoryId>, AsyncMethodRequestQueue)>,
        will_exit: Arc<AtomicBool>,
        sleep_time: Duration,
        abandoned_threshold_secs: i64,
    ) -> impl Stream<Item = Result<(RequestId, MegarepoAsynchronousRequestParams), MegarepoError>>
    {
        try_stream! {
            'outer: loop {
                let mut yielded = false;
                for (repo_ids, queue) in &queues_with_repos {
                    Self::cleanup_abandoned_requests(
                        &ctx,
                        repo_ids,
                        queue,
                        abandoned_threshold_secs
                    ).await?;
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

    async fn cleanup_abandoned_requests(
        ctx: &CoreContext,
        repo_ids: &[RepositoryId],
        queue: &AsyncMethodRequestQueue,
        abandoned_threshold_secs: i64,
    ) -> Result<(), MegarepoError> {
        let now = Timestamp::now();
        let abandoned_timestamp =
            Timestamp::from_timestamp_secs(now.timestamp_seconds() - abandoned_threshold_secs);
        let requests = queue
            .find_abandoned_requests(ctx, repo_ids, abandoned_timestamp)
            .await?;
        if !requests.is_empty() {
            ctx.scuba().clone().log_with_msg(
                "Find requests to abandon",
                Some(format!("{}", requests.len())),
            );
        }

        for req_id in requests {
            if queue
                .mark_abandoned_request_as_new(ctx, req_id.clone(), abandoned_timestamp)
                .await?
            {
                ctx.scuba()
                    .clone()
                    .add("request_id", req_id.0.0)
                    .log_with_msg("Abandoned request", None);
            }
        }
        Ok(())
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
        let target = params
            .target()?
            .clone()
            .into_config_format(&self.megarepo.mononoke())?;
        let queue = self
            .megarepo
            .async_method_request_queue(&ctx, &target)
            .await?;

        let ctx = self.prepare_ctx(&ctx, &req_id, &target);

        // Do the actual work.
        let work_fut = megarepo_async_request_compute(&ctx, &self.megarepo, params);

        // Start the loop that would keep saying that request is still being
        // processed
        let (keep_alive, keep_alive_abort_handle) = abortable({
            cloned!(ctx, req_id, queue);
            async move { Self::keep_alive_loop(&ctx, &req_id, &queue).await }
        });

        let keep_alive = tokio::spawn(keep_alive);

        pin_mut!(work_fut);
        pin_mut!(keep_alive);
        match select(work_fut, keep_alive).await {
            Either::Left((result, _)) => {
                // We completed the request - let's mark it as complete
                keep_alive_abort_handle.abort();
                info!(
                    ctx.logger(),
                    "[{}] request complete, saving result", &req_id.0
                );
                ctx.scuba()
                    .clone()
                    .log_with_msg("Request complete, saving result", None);

                // Save the result.
                let updated_res = queue.complete(&ctx, &req_id, result).await;

                let updated = match updated_res {
                    Ok(updated) => {
                        info!(ctx.logger(), "[{}] result saved", &req_id.0);
                        ctx.scuba().clone().log_with_msg("Result saved", None);
                        updated
                    }
                    Err(err) => {
                        ctx.scuba()
                            .clone()
                            .log_with_msg("Failed to save result", Some(format!("{:?}", err)));
                        return Err(err);
                    }
                };

                Ok(updated)
            }
            Either::Right((res, _)) => {
                // We haven't completed the request, and failed to update
                // inprogress timestamp. Most likely it means that other
                // worker has completed it

                res.map_err(MegarepoError::internal)?
                    .map_err(MegarepoError::internal)?;
                info!(
                    ctx.logger(),
                    "[{}] was completed by other worker, stopping", &req_id.0
                );
                Ok(false)
            }
        }
    }

    fn prepare_ctx(&self, ctx: &CoreContext, req_id: &RequestId, target: &Target) -> CoreContext {
        let ctx = ctx.with_mutated_scuba(|mut scuba| {
            scuba.add("request_id", req_id.0.0);
            scuba
        });

        info!(
            ctx.logger(),
            "[{}] new request:  id: {}, type: {}, repo_id: {}, bookmark: {}",
            &req_id.0,
            &req_id.0,
            &req_id.1,
            &target.repo_id,
            &target.bookmark,
        );

        ctx
    }

    async fn keep_alive_loop(
        ctx: &CoreContext,
        req_id: &RequestId,
        queue: &AsyncMethodRequestQueue,
    ) {
        loop {
            let mut scuba = ctx.scuba().clone();
            ctx.perf_counters().insert_perf_counters(&mut scuba);

            let res = queue.update_in_progress_timestamp(ctx, req_id).await;
            match res {
                Ok(res) => {
                    // Weren't able to update inprogress timestamp - that probably means
                    // that request was completed by someone else. Exiting
                    if !res {
                        scuba.log_with_msg(
                            "Race while updating inprogress timestamp, exiting keep-alive loop",
                            None,
                        );
                        break;
                    }
                    scuba.log_with_msg("Updated inprogress timestamp", None);
                }
                Err(err) => {
                    error!(
                        ctx.logger(),
                        "[{}] failed to update inprogress timestamp: {:?}", req_id.0, err
                    );
                    scuba.log_with_msg(
                        "Failed to update inprogress timestamp",
                        Some(format!("{:?}", err)),
                    );
                }
            }
            tokio::time::sleep(KEEP_ALIVE_INTERVAL).await;
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::Ordering;

    use anyhow::Error;
    use blobrepo::BlobRepo;
    use fbinit::FacebookInit;
    use mononoke_api::Mononoke;
    use requests_table::RequestType;
    use source_control as thrift;

    use super::*;

    #[fbinit::test]
    async fn test_request_stream_simple(fb: FacebookInit) -> Result<(), Error> {
        let q = AsyncMethodRequestQueue::new_test_in_memory().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let mononoke =
            Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

        let params = thrift::MegarepoSyncChangesetParams {
            cs_id: vec![],
            source_name: "name".to_string(),
            target: thrift::MegarepoTarget {
                repo_id: Some(0),
                bookmark: "book".to_string(),
                ..Default::default()
            },
            target_location: vec![],
            ..Default::default()
        };
        q.enqueue(ctx.clone(), &mononoke, params).await?;

        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx,
            ClaimedBy("name".to_string()),
            vec![(vec![RepositoryId::new(0)], q)],
            will_exit.clone(),
            Duration::from_millis(100),
            ABANDONED_REQUEST_THRESHOLD_SECS,
        );

        let s = tokio::spawn(s.try_collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await??;
        assert_eq!(res.len(), 1);
        assert_eq!(
            res[0].0.1,
            RequestType("megarepo_sync_changeset".to_string())
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_request_stream_clear_abandoned(fb: FacebookInit) -> Result<(), Error> {
        let q = AsyncMethodRequestQueue::new_test_in_memory().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let mononoke =
            Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

        let params = thrift::MegarepoSyncChangesetParams {
            cs_id: vec![],
            source_name: "name".to_string(),
            target: thrift::MegarepoTarget {
                repo_id: Some(0),
                bookmark: "book".to_string(),
                ..Default::default()
            },
            target_location: vec![],
            ..Default::default()
        };
        q.enqueue(ctx.clone(), &mononoke, params).await?;

        // Grab it from the queue...
        let dequed = q
            .dequeue(
                &ctx,
                &ClaimedBy("name".to_string()),
                &[RepositoryId::new(0)],
            )
            .await?;
        assert!(dequed.is_some());

        // ... and check that the queue is empty now...
        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx.clone(),
            ClaimedBy("name".to_string()),
            vec![(vec![RepositoryId::new(0)], q.clone())],
            will_exit.clone(),
            Duration::from_millis(100),
            ABANDONED_REQUEST_THRESHOLD_SECS,
        );

        let s = tokio::spawn(s.try_collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await??;
        assert_eq!(res, vec![]);

        // ... now make it "abandoned", and make sure we reclaim it
        tokio::time::sleep(Duration::from_secs(1)).await;
        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx,
            ClaimedBy("name".to_string()),
            vec![(vec![RepositoryId::new(0)], q)],
            will_exit.clone(),
            Duration::from_millis(100),
            1, // 1 second
        );

        let s = tokio::spawn(s.try_collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await??;
        assert_eq!(res.len(), 1);

        Ok(())
    }
}
