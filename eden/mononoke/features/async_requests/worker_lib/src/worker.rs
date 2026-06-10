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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests::AsyncRequestsError;
use async_requests::ClaimedBy;
use async_requests::DequeuedRequest;
use async_requests::RequestId;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::RowId;
use async_stream::stream;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use executor_lib::RepoShardedProcessExecutor;
use futures::Stream;
use futures::future::Either;
use futures::future::abortable;
use futures::future::select;
use futures::pin_mut;
use futures::stream::StreamExt;
use futures_stats::TimedFutureExt;
use hostname::get_hostname;
use megarepo_api::MegarepoApi;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_app::MononokeReposManager;
use mononoke_macros::mononoke;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use rand::RngExt as _;
use stats::define_stats;
use stats::prelude::*;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::methods::megarepo_async_request_compute;
use crate::scuba::log_result;
use crate::scuba::log_retriable_error;
use crate::scuba::log_start;
use crate::stats::stats_loop;

/// Decrement the refcount for an on-demand loaded repo. When the count drops
/// to zero the repo is removed from memory via `repos_mgr.remove_repo()`.
/// This is a standalone function so it can be called both through `&self` and
/// after `self` has been partially moved.
fn release_ondemand_repo_impl(
    repo_id: Option<RepositoryId>,
    ondemand_repo_refs: &Mutex<HashMap<RepositoryId, (String, usize)>>,
    repos_mgr: &MononokeReposManager<Repo>,
) {
    let repo_id = match repo_id {
        Some(id) => id,
        None => return,
    };

    let mut refs = ondemand_repo_refs.lock().expect("poisoned mutex");
    if let Some((name, count)) = refs.get_mut(&repo_id) {
        *count -= 1;
        if *count == 0 {
            let name = name.clone();
            refs.remove(&repo_id);
            drop(refs); // release lock before remove_repo
            info!("Removing on-demand repo {} (id={})", name, repo_id);
            repos_mgr.remove_repo(&name);
        }
    }
}

const DEQUEUE_STREAM_SLEEP_TIME: u64 = 1000;
// Number of seconds after which inprogress request is considered abandoned
// if it hasn't updated inprogress timestamp
const ABANDONED_REQUEST_THRESHOLD_SECS: i64 = 5 * 60;
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const CONCURRENCY_LIMIT_BACKOFF_BASE: Duration = Duration::from_secs(15);
const CONCURRENCY_LIMIT_BACKOFF_MAX_JITTER_SECS: u64 = 15;

define_stats! {
    prefix = "async_requests.worker";
    dequeue_called: timeseries("dequeue.called"; Count),
    cleanup_error: timeseries("cleanup.error"; Count),
    dequeue_error: timeseries("dequeue.error"; Count),
    process_aborted: timeseries("process.aborted"; Count),
    process_failed: timeseries("process.failed"; Count),
    requested: timeseries("requested"; Count),
}

#[derive(Clone)]
pub struct AsyncMethodRequestWorker {
    ctx: Arc<CoreContext>,
    repos_mgr: Arc<MononokeReposManager<Repo>>,
    mononoke: Arc<Mononoke<Repo>>,
    megarepo: Arc<MegarepoApi<Repo>>,
    name: String,
    queue: Arc<AsyncMethodRequestQueue>,
    will_exit: Arc<AtomicBool>,
    limit: Option<usize>,
    concurrency_limit: usize,
    /// Tracks the number of in-flight tasks using each on-demand loaded repo.
    /// Only call `repos_mgr.remove_repo()` when the count drops to zero.
    ondemand_repo_refs: Arc<Mutex<HashMap<RepositoryId, (String, usize)>>>,
}

impl AsyncMethodRequestWorker {
    pub async fn new(
        request_limit: Option<usize>,
        jobs: usize,
        ctx: Arc<CoreContext>,
        queue: Arc<AsyncMethodRequestQueue>,
        repos_mgr: Arc<MononokeReposManager<Repo>>,
        mononoke: Arc<Mononoke<Repo>>,
        megarepo: Arc<MegarepoApi<Repo>>,
        will_exit: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let name = {
            let tw_job_cluster = std::env::var("TW_JOB_CLUSTER");
            let tw_job_user = std::env::var("TW_JOB_USER");
            let tw_job_name = std::env::var("TW_JOB_NAME");
            let tw_task_id = std::env::var("TW_TASK_ID");
            match (tw_job_cluster, tw_job_user, tw_job_name, tw_task_id) {
                (Ok(tw_job_cluster), Ok(tw_job_user), Ok(tw_job_name), Ok(tw_task_id)) => {
                    format!("{tw_job_cluster}/{tw_job_user}/{tw_job_name}/{tw_task_id}")
                }
                _ => format!(
                    "async_requests_worker/{}",
                    get_hostname().unwrap_or_else(|_| "unknown_hostname".to_string())
                ),
            }
        };

        Ok(Self {
            ctx,
            repos_mgr,
            mononoke,
            megarepo,
            name,
            queue,
            will_exit,
            limit: request_limit,
            concurrency_limit: jobs,
            ondemand_repo_refs: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for AsyncMethodRequestWorker {
    /// Start async request worker.
    /// If limit is set the worker will process a preset number of requests and
    /// return. If the limit is None the worker will be running continuously. The
    /// will_exit atomic bool is a flag to prevent the worker from grabbing new
    /// items from the queue and gracefully terminate.
    async fn execute(&self) -> Result<()> {
        // Start the stats logger loop
        let (stats, stats_abort_handle) = abortable({
            cloned!(self.ctx, self.queue);
            let repo_ids = self.mononoke.known_repo_ids().clone();
            async move { stats_loop(&ctx, repo_ids, &queue).await }
        });
        let _stats = mononoke::spawn_task(stats);

        // Build stream that pools all the queues
        let request_stream = self
            .request_stream(&self.ctx, self.queue.clone(), self.will_exit.clone())
            .boxed();

        let request_stream = if let Some(limit) = self.limit {
            request_stream.take(limit).left_stream()
        } else {
            request_stream.right_stream()
        };

        info!("Worker initialization complete, starting request processing loop.",);

        request_stream
            .for_each_concurrent(Some(self.concurrency_limit), |dequeued| async move {
                let worker = self.clone();
                let ctx = CoreContext::clone(&self.ctx);
                if let Err(e) = mononoke::spawn_task(worker.compute_and_mark_completed(
                    ctx,
                    dequeued.id,
                    dequeued.repo_id,
                    dequeued.params,
                    dequeued.root_request_id,
                    dequeued.created_by,
                ))
                .await
                {
                    warn!("Error spawning request: {:?}", e);
                }
            })
            .await;

        info!("Worker exiting");

        stats_abort_handle.abort();

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Worker stopping");

        Ok(())
    }
}

impl AsyncMethodRequestWorker {
    pub fn request_stream(
        &self,
        ctx: &CoreContext,
        queue: Arc<AsyncMethodRequestQueue>,
        will_exit: Arc<AtomicBool>,
    ) -> impl Stream<Item = DequeuedRequest> + use<> {
        let claimed_by = ClaimedBy(self.name.clone());
        let sleep_time = Duration::from_millis(DEQUEUE_STREAM_SLEEP_TIME);
        Self::request_stream_inner(
            ctx.clone(),
            claimed_by,
            queue,
            will_exit,
            sleep_time,
            ABANDONED_REQUEST_THRESHOLD_SECS,
        )
    }

    fn request_stream_inner(
        ctx: CoreContext,
        claimed_by: ClaimedBy,
        queue: Arc<AsyncMethodRequestQueue>,
        will_exit: Arc<AtomicBool>,
        sleep_time: Duration,
        abandoned_threshold_secs: i64,
    ) -> impl Stream<Item = DequeuedRequest> {
        stream! {
            loop {
                STATS::dequeue_called.add_value(1);

                if let Err(e) =
                    Self::cleanup_abandoned_requests(&ctx, &queue, abandoned_threshold_secs).await
                {
                    STATS::cleanup_error.add_value(1);
                    warn!("error while cleaning up abandoned requests, skipping: {}", e);
                };

                if will_exit.load(Ordering::Relaxed) {
                    break;
                }

                match queue.dequeue(&ctx, &claimed_by).await {
                    Err(e) => {
                        STATS::dequeue_error.add_value(1);
                        warn!("error while dequeueing, skipping: {:?}", e);
                        tokio::time::sleep(sleep_time).await;
                    }
                    Ok(Some(dequeued)) => {
                        yield dequeued;
                    }
                    Ok(None) => {
                        // No requests in the queues, sleep before trying again.
                        debug!("nothing to do, sleeping",);
                        tokio::time::sleep(sleep_time).await;
                    }
                }
            }
        }
    }

    async fn cleanup_abandoned_requests(
        ctx: &CoreContext,
        queue: &AsyncMethodRequestQueue,
        abandoned_threshold_secs: i64,
    ) -> Result<(), AsyncRequestsError> {
        let now = Timestamp::now();
        let abandoned_timestamp =
            Timestamp::from_timestamp_secs(now.timestamp_seconds() - abandoned_threshold_secs);
        let requests = queue
            .find_abandoned_requests(ctx, abandoned_timestamp)
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

    /// Ensure the repo for this request is loaded. Returns true if this is
    /// an on-demand repo that should be released after processing via
    /// `release_ondemand_repo`.
    async fn ensure_repo_loaded(&self, repo_id: Option<RepositoryId>) -> Result<bool, Error> {
        let repo_id = match repo_id {
            Some(id) => id,
            None => return Ok(false),
        };

        // Fast path: check if already tracked as on-demand or pre-configured.
        {
            let mut refs = self.ondemand_repo_refs.lock().expect("poisoned mutex");
            if let Some((_, count)) = refs.get_mut(&repo_id) {
                *count += 1;
                return Ok(true);
            }
            // Pre-configured repo — no on-demand management needed.
            if self.mononoke.raw_repo_by_id(repo_id.id()).is_some() {
                return Ok(false);
            }
            // Insert a sentinel entry with refcount 1 while we hold the lock.
            // This prevents a concurrent task from also calling add_repo for
            // the same repo_id — it will see the entry and just bump the count.
            let configs = self.repos_mgr.configs();
            let (repo_name, _) = configs
                .get_or_load_repo_config_by_id(repo_id.id())
                .map_err(|e| Error::msg(format!("No config found for repo_id {repo_id}: {e}")))?;
            refs.insert(repo_id, (repo_name, 1));
        }

        // Load the repo outside the lock. If this fails we must clean up the
        // sentinel so subsequent tasks don't think the repo is loaded.
        let repo_name = {
            let refs = self.ondemand_repo_refs.lock().expect("poisoned mutex");
            refs.get(&repo_id).map(|(name, _)| name.clone()).unwrap()
        };
        info!("Loading repo {} (id={}) on-demand", repo_name, repo_id);
        if let Err(e) = self.repos_mgr.add_repo(&repo_name).await {
            // Remove the sentinel so concurrent waiters don't see a loaded repo.
            let mut refs = self.ondemand_repo_refs.lock().expect("poisoned mutex");
            refs.remove(&repo_id);
            return Err(e);
        }

        Ok(true)
    }

    /// Decrement the refcount for an on-demand repo. When it reaches zero,
    /// remove the repo from memory.
    fn release_ondemand_repo(&self, repo_id: Option<RepositoryId>) {
        release_ondemand_repo_impl(repo_id, &self.ondemand_repo_refs, &self.repos_mgr);
    }

    fn concurrency_limit_backoff() -> Duration {
        let jitter_secs = rand::rng().random_range(0..=CONCURRENCY_LIMIT_BACKOFF_MAX_JITTER_SECS);
        CONCURRENCY_LIMIT_BACKOFF_BASE + Duration::from_secs(jitter_secs)
    }

    /// Params into stored response. Doesn't mark it as "in progress" (as this is done during dequeueing).
    /// Returns true if the result was successfully stored. Returns false if we
    /// lost the race (the request table was updated).
    async fn compute_and_mark_completed(
        self,
        ctx: CoreContext,
        req_id: RequestId,
        repo_id: Option<RepositoryId>,
        params: AsynchronousRequestParams,
        root_request_id: Option<RowId>,
        created_by: Option<String>,
    ) {
        // Load the repo on-demand if needed
        let is_ondemand = match self.ensure_repo_loaded(repo_id).await {
            Ok(v) => v,
            Err(err) => {
                STATS::process_failed.add_value(1);
                error!("[{}] Failed to load repo for request: {:?}", &req_id.0, err);
                if let Err(e) = self.queue.requeue(&ctx, req_id).await {
                    error!("Failed to requeue after repo load failure: {:?}", e);
                }
                return;
            }
        };
        let cleanup_repo_id = if is_ondemand { repo_id } else { None };

        let target = match params.target() {
            Ok(target) => target,
            Err(err) => {
                STATS::process_failed.add_value(1);
                error!("Error getting target: {:?}", err);
                self.release_ondemand_repo(cleanup_repo_id);
                return;
            }
        };
        let ctx = self.prepare_ctx(
            &ctx,
            &req_id,
            &target,
            root_request_id,
            created_by.as_deref(),
        );

        // Check concurrency limit for this request type. If exceeded,
        // hold the claim briefly before requeueing so the same hot request
        // does not immediately churn across the whole worker fleet.
        match self.queue.concurrency_limit_reached(&ctx, &req_id.1).await {
            Ok(true) => {
                let row_id = req_id.0;
                let backoff = Self::concurrency_limit_backoff();
                info!(
                    "[{}] Concurrency limit reached for {}, backing off for {:?}",
                    &row_id, &req_id.1.0, backoff,
                );
                ctx.scuba()
                    .clone()
                    .add("backoff_ms", backoff.as_millis() as i64)
                    .log_with_msg("Request throttled by concurrency limit", None);
                tokio::time::sleep(backoff).await;
                if let Err(requeue_err) = self.queue.requeue(&ctx, req_id).await {
                    error!(
                        "[{}] Failed to requeue request after concurrency limit: {:?}",
                        &row_id, requeue_err
                    );
                }
                self.release_ondemand_repo(cleanup_repo_id);
                return;
            }
            Err(e) => {
                error!("[{}] Failed to check concurrency limit: {:?}", &req_id.0, e);
            }
            _ => {}
        }

        log_start(&ctx);

        // Save refs for cleanup after self is partially moved.
        let ondemand_repo_refs = self.ondemand_repo_refs.clone();
        let repos_mgr_for_cleanup = self.repos_mgr.clone();
        // Do the actual work.
        STATS::requested.add_value(1);
        let work_fut = megarepo_async_request_compute(
            &ctx,
            self.mononoke,
            &self.megarepo,
            &self.queue,
            &req_id.0,
            root_request_id,
            params,
            created_by,
        )
        .timed();

        // Start the loop that would keep saying that request is still being
        // processed
        let (keep_alive, keep_alive_abort_handle) = abortable({
            cloned!(ctx, req_id, self.queue);
            async move { Self::keep_alive_loop(&ctx, &req_id, &queue).await }
        });

        let keep_alive = mononoke::spawn_task(keep_alive);

        pin_mut!(work_fut);
        pin_mut!(keep_alive);
        match select(work_fut, keep_alive).await {
            Either::Left(((stats, result), _)) => {
                // We completed the request - let's mark it as complete
                keep_alive_abort_handle.abort();
                info!(
                    "[{}] request complete, saving result (processed: {})",
                    &req_id.0,
                    result.is_ok()
                );

                // Save the result.
                match result {
                    Ok(work_result) => {
                        let complete_result = self
                            .queue
                            .complete(&ctx, &req_id, work_result.clone())
                            .await;
                        log_result(ctx.clone(), &stats, &work_result, &complete_result);
                        match complete_result {
                            Ok(updated) => {
                                info!("[{}] result saved (updated: {})", &req_id.0, updated);
                            }
                            Err(err) => {
                                error!("[{}] failed to save success result: {:?}", &req_id.0, err);
                            }
                        };
                    }
                    Err(err) => {
                        let err_result = self.queue.retry(&ctx, &req_id).await;
                        match err_result {
                            Ok(will_retry) => {
                                if will_retry {
                                    info!(
                                        "[{}] worker failed to process request, will retry: {:?}",
                                        &req_id.0, err
                                    );
                                } else {
                                    // Retries exhausted — failure has been cascaded
                                    // to any dependent requests atomically.
                                    info!(
                                        "[{}] worker failed to process request, maximum retry attempts reached, failure cascaded: {:?}",
                                        &req_id.0, err
                                    );
                                }
                            }
                            Err(err) => {
                                error!(
                                    "[{}] failed to process retry attempt: {:?}",
                                    &req_id.0, err
                                );
                            }
                        }

                        log_retriable_error(ctx.clone(), &stats, err);
                    }
                }
            }
            Either::Right((_, _)) => {
                // We haven't completed the request, and failed to update
                // inprogress timestamp. Most likely it means that other
                // worker has completed it

                STATS::process_aborted.add_value(1);
                info!("[{}] was completed by other worker, stopping", &req_id.0);
            }
        }

        // Release on-demand repo (decrement refcount, remove when zero).
        release_ondemand_repo_impl(cleanup_repo_id, &ondemand_repo_refs, &repos_mgr_for_cleanup);
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
                        "[{}] failed to update inprogress timestamp: {:?}",
                        req_id.0, err
                    );
                    scuba.log_with_msg(
                        "Failed to update inprogress timestamp",
                        Some(format!("{err:?}")),
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
    use fbinit::FacebookInit;
    use mononoke_api::RepositoryId;
    use mononoke_macros::mononoke;
    use requests_table::RequestType;
    use source_control as thrift;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_request_stream_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo_id = RepositoryId::new(0);
        let q = Arc::new(AsyncMethodRequestQueue::new_test_in_memory().unwrap());
        let ctx = CoreContext::test_mock(fb);

        let params = thrift::MegarepoSyncChangesetParams {
            cs_id: vec![],
            source_name: "name".to_string(),
            target: thrift::MegarepoTarget {
                repo_id: Some(repo_id.id() as i64),
                bookmark: "book".to_string(),
                ..Default::default()
            },
            target_location: vec![],
            ..Default::default()
        };
        q.enqueue(&ctx, Some(&repo_id), params).await?;

        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx.clone(),
            ClaimedBy("name".to_string()),
            q,
            will_exit.clone(),
            Duration::from_millis(100),
            ABANDONED_REQUEST_THRESHOLD_SECS,
        );

        let s = mononoke::spawn_task(s.collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await?;
        assert_eq!(res.len(), 1);
        assert_eq!(
            res[0].id.1,
            RequestType("megarepo_sync_changeset".to_string())
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_request_stream_clear_abandoned(fb: FacebookInit) -> Result<(), Error> {
        let repo_id = RepositoryId::new(0);
        let q = Arc::new(AsyncMethodRequestQueue::new_test_in_memory().unwrap());
        let ctx = CoreContext::test_mock(fb);

        let params = thrift::MegarepoSyncChangesetParams {
            cs_id: vec![],
            source_name: "name".to_string(),
            target: thrift::MegarepoTarget {
                repo_id: Some(repo_id.id() as i64),
                bookmark: "book".to_string(),
                ..Default::default()
            },
            target_location: vec![],
            ..Default::default()
        };
        q.enqueue(&ctx, Some(&repo_id), params).await?;

        // Grab it from the queue...
        let dequeued = q.dequeue(&ctx, &ClaimedBy("name".to_string())).await?;
        assert!(dequeued.is_some());

        // ... and check that the queue is empty now...
        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx.clone(),
            ClaimedBy("name".to_string()),
            q.clone(),
            will_exit.clone(),
            Duration::from_millis(100),
            ABANDONED_REQUEST_THRESHOLD_SECS,
        );

        let s = mononoke::spawn_task(s.collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await?;
        assert_eq!(res, vec![]);

        // ... now make it "abandoned", and make sure we reclaim it
        tokio::time::sleep(Duration::from_secs(1)).await;
        let will_exit = Arc::new(AtomicBool::new(false));
        let s = AsyncMethodRequestWorker::request_stream_inner(
            ctx.clone(),
            ClaimedBy("name".to_string()),
            q,
            will_exit.clone(),
            Duration::from_millis(100),
            1, // 1 second
        );

        let s = mononoke::spawn_task(s.collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(1)).await;
        will_exit.store(true, Ordering::Relaxed);
        let res = s.await?;
        assert_eq!(res.len(), 1);

        Ok(())
    }
}
