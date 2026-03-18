/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use blobstore::Blobstore;
use blobstore::PutBehaviour;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use memblob::Memblob;
use mononoke_api::MononokeRepo;
use mononoke_types::BlobstoreKey as BlobstoreKeyTrait;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use requests_table::BlobstoreKey;
pub use requests_table::ClaimedBy;
use requests_table::LongRunningRequestEntry;
use requests_table::LongRunningRequestsQueue;
use requests_table::QueueStats;
pub use requests_table::RequestId;
use requests_table::RequestType;
pub use requests_table::RowId;
use requests_table::SqlLongRunningRequestsQueue;
use sql_construct::SqlConstruct;
use stats::define_stats;
use stats::prelude::TimeseriesStatic;

use crate::AsyncRequestsError;
use crate::types::AsynchronousRequestParams;
use crate::types::AsynchronousRequestResult;
use crate::types::DeriveBackfill;
use crate::types::DeriveBackfillRepo;
use crate::types::DeriveBoundaries;
use crate::types::DeriveSlice;
use crate::types::Request;
use crate::types::ThriftParams;
use crate::types::Token;

const INITIAL_POLL_DELAY_MS: u64 = 1000;
const MAX_POLL_DURATION: Duration = Duration::from_mins(1);
const JK_RETRY_LIMIT: &str = "scm/mononoke:async_requests_retry_limit";

/// A request that has been dequeued from the queue and is ready to be processed.
#[derive(Debug, PartialEq)]
pub struct DequeuedRequest {
    pub id: RequestId,
    pub params: AsynchronousRequestParams,
    pub root_request_id: Option<RowId>,
}

/// JustKnob for maximum concurrent requests across all workers.
/// The switch/entity determines the limit: types in a concurrency group
/// share a switch (and thus a limit), while ungrouped types use their
/// own name as the switch. A value <= 0 means no limit.
const JK_MAX_CONCURRENT: &str = "scm/mononoke:async_requests_max_concurrent";

/// Returns the JustKnob switch key and the request types to count for
/// concurrency limiting. Grouped types share a switch and count across
/// all members; ungrouped types use their own name.
fn concurrency_key(request_type: &str) -> (&str, Vec<&str>) {
    const DERIVE_BACKFILL: &[&str] = &[
        DeriveBoundaries::NAME,
        DeriveSlice::NAME,
        DeriveBackfillRepo::NAME,
        DeriveBackfill::NAME,
    ];
    if DERIVE_BACKFILL.contains(&request_type) {
        ("derive_backfill", DERIVE_BACKFILL.to_vec())
    } else {
        (request_type, vec![request_type])
    }
}

define_stats! {
    prefix = "async_requests.queue";
    complete_called: timeseries("complete.called"; Count),
    complete_error: timeseries("complete.error"; Count),
    complete_success: timeseries("complete.success"; Count),
    retry_called: timeseries("retry.called"; Count),
    retry_error: timeseries("retry.error"; Count),
    retry_success: timeseries("retry.success"; Count),
    retry_exceeded: timeseries("retry.exceeded"; Count),
    dequeue_called: timeseries("dequeue.called"; Count),
    dequeue_error: timeseries("dequeue.error"; Count),
    dequeue_success: timeseries("dequeue.success"; Count),
    enqueue_called: timeseries("enqueue.called"; Count),
    enqueue_error: timeseries("enqueue.error"; Count),
    enqueue_success: timeseries("enqueue.success"; Count),
    poll_called: timeseries("poll.called"; Count),
    poll_error: timeseries("poll.error"; Count),
    poll_empty: timeseries("poll.empty"; Count),
    poll_success: timeseries("poll.success"; Count),
    poll_timeout: timeseries("poll.timeout"; Count),
}

#[derive(Clone)]
pub struct AsyncMethodRequestQueue {
    blobstore: Arc<dyn Blobstore>,
    table: Arc<dyn LongRunningRequestsQueue>,
    repos: Option<Vec<RepositoryId>>,
}

#[derive(Debug)]
pub enum PollError {
    Poll(Error),
    Fatal(AsyncRequestsError),
}

impl From<PollError> for Error {
    fn from(err: PollError) -> Self {
        match err {
            PollError::Poll(e) => e,
            PollError::Fatal(e) => e.into(),
        }
    }
}

impl AsyncMethodRequestQueue {
    pub fn new(
        table: Arc<dyn LongRunningRequestsQueue>,
        blobstore: Arc<dyn Blobstore>,
        repos: Option<Vec<RepositoryId>>,
    ) -> Self {
        Self {
            blobstore,
            table,
            repos,
        }
    }

    pub fn new_test_in_memory(repos: Option<Vec<RepositoryId>>) -> Result<Self, Error> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::new(PutBehaviour::IfAbsent));
        let table: Arc<dyn LongRunningRequestsQueue> =
            Arc::new(SqlLongRunningRequestsQueue::with_sqlite_in_memory()?);

        Ok(Self {
            blobstore,
            table,
            repos,
        })
    }

    /// Check if we're under the concurrency limit for the given request type.
    /// Types in a concurrency group share a limit; ungrouped types have
    /// their own.
    /// Returns false if we're under the limit, true if we're over.
    pub async fn concurrency_limit_reached(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
    ) -> Result<bool, Error> {
        let (switch, count_types) = concurrency_key(&request_type.0);

        let max_concurrent = justknobs::get_as::<i64>(JK_MAX_CONCURRENT, Some(switch))?;

        if max_concurrent <= 0 {
            return Ok(false);
        }

        let type_refs: Vec<&str> = count_types.to_vec();
        let current_count = self
            .table
            .count_inprogress_by_types(ctx, &type_refs)
            .await?;

        // We need to have dequeued the new request to know its type, so
        // it will be included in the count. So this check is > rather than
        // >= to allow for this.
        if current_count > max_concurrent {
            return Ok(true);
        }

        Ok(false)
    }

    pub async fn enqueue<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        thrift_params: P,
    ) -> Result<<P::R as Request>::Token, Error> {
        STATS::enqueue_called.add_value(1);
        let request_type = RequestType(P::R::NAME.to_owned());
        let rust_params = thrift_params.into();
        self.enqueue_inner::<P>(ctx, repo_id, request_type, rust_params)
            .await
            .inspect(|_token| {
                STATS::enqueue_success.add_value(1);
            })
            .inspect_err(|_err| {
                STATS::enqueue_error.add_value(1);
            })
    }

    async fn enqueue_inner<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        request_type: RequestType,
        rust_params: AsynchronousRequestParams,
    ) -> Result<<P::R as Request>::Token, Error> {
        let params_object_id = rust_params.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(params_object_id.blobstore_key());
        let table_id = self
            .table
            .add_request(ctx, &request_type, repo_id, &blobstore_key)
            .await?;
        let token = <P::R as Request>::Token::from_db_id(table_id)?;
        Ok(token)
    }

    /// Enqueue a request with optional dependencies on other requests.
    /// The request will remain in 'new' status until all dependencies reach 'ready' or 'polled'.
    pub async fn enqueue_with_dependencies<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        thrift_params: P,
        depends_on: &[RowId],
    ) -> Result<<P::R as Request>::Token, Error> {
        STATS::enqueue_called.add_value(1);
        let request_type = RequestType(P::R::NAME.to_owned());
        let rust_params = thrift_params.into();
        self.enqueue_with_dependencies_inner::<P>(
            ctx,
            repo_id,
            request_type,
            rust_params,
            depends_on,
        )
        .await
        .inspect(|_token| {
            STATS::enqueue_success.add_value(1);
        })
        .inspect_err(|_err| {
            STATS::enqueue_error.add_value(1);
        })
    }

    async fn enqueue_with_dependencies_inner<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        request_type: RequestType,
        rust_params: AsynchronousRequestParams,
        depends_on: &[RowId],
    ) -> Result<<P::R as Request>::Token, Error> {
        let params_object_id = rust_params.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(params_object_id.blobstore_key());

        let table_id = self
            .table
            .add_request_with_dependencies(ctx, &request_type, repo_id, &blobstore_key, depends_on)
            .await?;

        let token = <P::R as Request>::Token::from_db_id(table_id)?;
        Ok(token)
    }

    /// Enqueue a request with a root_request_id linking it to a parent request.
    pub async fn enqueue_with_root<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        thrift_params: P,
        root_request_id: &RowId,
    ) -> Result<<P::R as Request>::Token, Error> {
        STATS::enqueue_called.add_value(1);
        let request_type = RequestType(P::R::NAME.to_owned());
        let rust_params = thrift_params.into();
        let params_object_id = rust_params.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(params_object_id.blobstore_key());
        let table_id = self
            .table
            .add_request_with_root(ctx, &request_type, repo_id, &blobstore_key, root_request_id)
            .await?;
        let token = <P::R as Request>::Token::from_db_id(table_id)?;
        STATS::enqueue_success.add_value(1);
        Ok(token)
    }

    /// Enqueue a request with dependencies and a root_request_id.
    pub async fn enqueue_with_dependencies_and_root<P: ThriftParams>(
        &self,
        ctx: &CoreContext,
        repo_id: Option<&RepositoryId>,
        thrift_params: P,
        depends_on: &[RowId],
        root_request_id: &RowId,
    ) -> Result<<P::R as Request>::Token, Error> {
        STATS::enqueue_called.add_value(1);
        let request_type = RequestType(P::R::NAME.to_owned());
        let rust_params = thrift_params.into();
        let params_object_id = rust_params.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(params_object_id.blobstore_key());
        let table_id = self
            .table
            .add_request_with_dependencies_and_root(
                ctx,
                &request_type,
                repo_id,
                &blobstore_key,
                depends_on,
                root_request_id,
            )
            .await?;
        let token = <P::R as Request>::Token::from_db_id(table_id)?;
        STATS::enqueue_success.add_value(1);
        Ok(token)
    }

    pub async fn dequeue(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
    ) -> Result<Option<DequeuedRequest>, Error> {
        STATS::dequeue_called.add_value(1);
        self.dequeue_inner(ctx, claimed_by)
            .await
            .inspect(|_token| {
                STATS::dequeue_success.add_value(1);
            })
            .inspect_err(|_err| {
                STATS::dequeue_error.add_value(1);
            })
    }

    pub async fn dequeue_inner(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
    ) -> Result<Option<DequeuedRequest>, Error> {
        let entry = self
            .table
            .claim_and_get_new_request(ctx, claimed_by, self.repos.as_deref())
            .await?;

        if let Some(entry) = entry {
            let thrift_params = AsynchronousRequestParams::load_from_key(
                ctx,
                &self.blobstore,
                &entry.args_blobstore_key.0,
            )
            .await?;
            let root_request_id = entry.root_request_id.clone();
            let req_id = RequestId(entry.id, entry.request_type);
            Ok(Some(DequeuedRequest {
                id: req_id,
                params: thrift_params,
                root_request_id,
            }))
        } else {
            // empty queue
            Ok(None)
        }
    }

    pub async fn complete(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        result: AsynchronousRequestResult,
    ) -> Result<bool, Error> {
        STATS::complete_called.add_value(1);
        self.complete_inner(ctx, req_id, result)
            .await
            .inspect(|_token| {
                STATS::complete_success.add_value(1);
            })
            .inspect_err(|_err| {
                STATS::complete_error.add_value(1);
            })
    }

    pub async fn complete_inner(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        result: AsynchronousRequestResult,
    ) -> Result<bool, Error> {
        let result_object_id = result.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(result_object_id.blobstore_key());
        self.table.mark_ready(ctx, req_id, blobstore_key).await
    }

    pub async fn retry(&self, ctx: &CoreContext, req_id: &RequestId) -> Result<bool, Error> {
        STATS::retry_called.add_value(1);
        let max_retry_allowed = justknobs::get_as::<u8>(JK_RETRY_LIMIT, Some(&req_id.1.0))?;

        self.table
            .update_for_retry_or_fail(ctx, req_id, max_retry_allowed)
            .await
            .inspect(|will_retry| {
                if *will_retry {
                    STATS::retry_success.add_value(1);
                } else {
                    STATS::retry_exceeded.add_value(1);
                }
            })
            .inspect_err(|_err| {
                STATS::retry_error.add_value(1);
            })
    }

    async fn poll_once<R: Request>(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<<R as Request>::ThriftResult>, PollError> {
        let maybe_result_blobstore_key = match self
            .table
            .poll(ctx, req_id)
            .await
            .map_err(PollError::Poll)?
        {
            None => return Ok(None),
            Some((_, entry)) => entry.result_blobstore_key,
        };

        let result_blobstore_key = match maybe_result_blobstore_key {
            Some(rbk) => rbk,
            None => {
                return Err(PollError::Fatal(anyhow!(
                    "Programming error: successful poll with empty result_blobstore_key for {:?}",
                    req_id
                ).into()));
            }
        };

        let result: AsynchronousRequestResult =
            AsynchronousRequestResult::load_from_key(ctx, &self.blobstore, &result_blobstore_key.0)
                .await
                .map_err(PollError::Fatal)?;
        match result.try_into() {
            Ok(res) => Ok(Some(res)),
            Err(e) => Err(PollError::Fatal(e)),
        }
    }

    /// Mark a request as failed and cascade the failure to all dependent requests.
    /// This is safe to call for any request type — if there are no dependents,
    /// the cascade is a no-op.
    pub async fn mark_failed_with_cascade(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool, Error> {
        self.table.mark_failed_with_cascade(ctx, &req_id.0).await
    }

    pub async fn poll<T: Token>(
        &self,
        ctx: &CoreContext,
        token: T,
    ) -> Result<<T::R as Request>::PollResponse, PollError> {
        STATS::poll_called.add_value(1);
        self.poll_inner(ctx, token)
            .await
            // we don't bump poll_success here, we do it in poll_inner
            .inspect_err(|_err| {
                STATS::poll_error.add_value(1);
            })
    }

    pub async fn poll_inner<T: Token>(
        &self,
        ctx: &CoreContext,
        token: T,
    ) -> Result<<T::R as Request>::PollResponse, PollError> {
        let mut backoff_ms = INITIAL_POLL_DELAY_MS;
        let before = Instant::now();
        let row_id = token.to_db_id().map_err(PollError::Fatal)?;
        let req_id = RequestId(row_id, RequestType(T::R::NAME.to_owned()));

        loop {
            let maybe_thrift_result: Option<<T::R as Request>::ThriftResult> =
                self.poll_once::<T::R>(ctx, &req_id).await?;
            let next_sleep = Duration::from_millis(rand::random::<u64>() % backoff_ms);

            match maybe_thrift_result {
                Some(thrift_result) => {
                    // Nice, the result is ready!
                    STATS::poll_success.add_value(1);
                    return Ok(<T::R as Request>::thrift_result_into_poll_response(
                        thrift_result,
                    ));
                }
                None if before.elapsed() + next_sleep > MAX_POLL_DURATION => {
                    // The result is not yet ready, but we're out of time
                    STATS::poll_timeout.add_value(1);
                    return Ok(T::R::empty_poll_response());
                }
                None => {
                    // The result is not yet ready and we can wait a little longer
                    tokio::time::sleep(next_sleep).await;
                    backoff_ms *= 2;
                }
            }
        }
    }

    pub async fn update_in_progress_timestamp(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool, Error> {
        self.table.update_in_progress_timestamp(ctx, req_id).await
    }

    pub async fn find_abandoned_requests(
        &self,
        ctx: &CoreContext,
        abandoned_timestamp: Timestamp,
    ) -> Result<Vec<RequestId>, Error> {
        self.table
            .find_abandoned_requests(ctx, self.repos.as_deref(), abandoned_timestamp)
            .await
    }

    pub async fn mark_abandoned_request_as_new(
        &self,
        ctx: &CoreContext,
        request_id: RequestId,
        abandoned_timestamp: Timestamp,
    ) -> Result<bool, Error> {
        self.table
            .mark_abandoned_request_as_new(ctx, request_id, abandoned_timestamp)
            .await
    }

    pub async fn requeue(&self, ctx: &CoreContext, request_id: RequestId) -> Result<bool, Error> {
        self.table.mark_new(ctx, &request_id).await
    }

    pub async fn list_requests(
        &self,
        ctx: &CoreContext,
        last_update_newer_than: Option<&Timestamp>,
        fatal_errors: bool,
    ) -> Result<
        Vec<(
            RequestId,
            LongRunningRequestEntry,
            AsynchronousRequestParams,
        )>,
        Error,
    > {
        let entries = self
            .table
            .list_requests(ctx, self.repos.as_deref(), last_update_newer_than)
            .await
            .context("listing requests from the DB")?;

        let results = stream::iter(entries)
            .map(|entry| async {
                let thrift_params = AsynchronousRequestParams::load_from_key(
                    ctx,
                    &self.blobstore,
                    &entry.args_blobstore_key.0,
                )
                .await
                .context("deserializing")?;
                let req_id = RequestId(entry.id.clone(), entry.request_type.clone());
                Ok::<_, Error>((req_id, entry, thrift_params))
            })
            .buffer_unordered(10);

        if fatal_errors {
            results.try_collect().await
        } else {
            Ok(results
                .inspect_err(|err| println!("Error: {:?}, skipping", err))
                .then(|entry| async { stream::iter(entry.into_iter()) })
                .flatten()
                .collect::<Vec<(
                    RequestId,
                    LongRunningRequestEntry,
                    AsynchronousRequestParams,
                )>>()
                .await)
        }
    }

    pub async fn get_request_by_id(
        &self,
        ctx: &CoreContext,
        row_id: &RowId,
    ) -> Result<
        Option<(
            RequestId,
            LongRunningRequestEntry,
            AsynchronousRequestParams,
            Option<AsynchronousRequestResult>,
        )>,
        Error,
    > {
        let entry = self.table.test_get_request_entry_by_id(ctx, row_id).await?;

        if let Some(entry) = entry {
            let thrift_params = AsynchronousRequestParams::load_from_key(
                ctx,
                &self.blobstore,
                &entry.args_blobstore_key.0,
            )
            .await?;
            let req_id = RequestId(entry.id.clone(), entry.request_type.clone());
            let thrift_result = if let Some(result_blobstore_key) = &entry.result_blobstore_key {
                Some(
                    AsynchronousRequestResult::load_from_key(
                        ctx,
                        &self.blobstore,
                        &result_blobstore_key.0,
                    )
                    .await?,
                )
            } else {
                None
            };
            Ok(Some((req_id, entry, thrift_params, thrift_result)))
        } else {
            // empty queue
            Ok(None)
        }
    }

    /// Get all requests that share a given root_request_id, with their params loaded.
    pub async fn get_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<
        Vec<(
            RequestId,
            LongRunningRequestEntry,
            AsynchronousRequestParams,
            Option<AsynchronousRequestResult>,
        )>,
        Error,
    > {
        let entries = self
            .table
            .get_requests_by_root_id(ctx, root_request_id)
            .await?;

        stream::iter(entries)
            .map(|entry| async {
                let thrift_params = AsynchronousRequestParams::load_from_key(
                    ctx,
                    &self.blobstore,
                    &entry.args_blobstore_key.0,
                )
                .await?;
                let req_id = RequestId(entry.id.clone(), entry.request_type.clone());
                let thrift_result = if let Some(ref result_key) = entry.result_blobstore_key {
                    Some(
                        AsynchronousRequestResult::load_from_key(
                            ctx,
                            &self.blobstore,
                            &result_key.0,
                        )
                        .await?,
                    )
                } else {
                    None
                };
                Ok::<_, Error>((req_id, entry, thrift_params, thrift_result))
            })
            .buffer_unordered(100)
            .try_collect()
            .await
    }

    /// Mark all 'new' requests with the given root_request_id as 'failed'.
    pub async fn fail_new_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<u64, Error> {
        self.table
            .fail_new_requests_by_root_id(ctx, root_request_id)
            .await
    }

    pub async fn get_queue_stats(
        &self,
        ctx: &CoreContext,
        exclude_backfill: bool,
    ) -> Result<QueueStats, Error> {
        self.table
            .get_queue_stats(ctx, self.repos.as_deref(), exclude_backfill)
            .await
    }
}

#[cfg(test)]
mod tests {
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::FutureExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use maplit::hashmap;
    use mononoke_api::Repo;
    use mononoke_macros::mononoke;
    use repo_identity::RepoIdentityRef;
    use requests_table::ClaimedBy;
    use requests_table::RequestStatus;
    use source_control::MegarepoAddBranchingTargetParams as ThriftMegarepoAddBranchingTargetParams;
    use source_control::MegarepoAddBranchingTargetResult;
    use source_control::MegarepoAddTargetParams as ThriftMegarepoAddTargetParams;
    use source_control::MegarepoAddTargetResult;
    use source_control::MegarepoChangeTargetConfigParams as ThriftMegarepoChangeTargetConfigParams;
    use source_control::MegarepoChangeTargetConfigResult;
    use source_control::MegarepoRemergeSourceParams as ThriftMegarepoRemergeSourceParams;
    use source_control::MegarepoRemergeSourceResult;
    use source_control::MegarepoSyncChangesetParams as ThriftMegarepoSyncChangesetParams;
    use source_control::MegarepoSyncChangesetResult;
    use source_control::MegarepoSyncTargetConfig as ThriftMegarepoSyncTargetConfig;
    use source_control::MegarepoTarget as ThriftMegarepoTarget;
    use source_control::RepoSpecifier;

    use super::*;
    use crate::types::AsynchronousRequestResult;
    use crate::types::MegarepoAddBranchingSyncTarget;
    use crate::types::MegarepoAddSyncTarget;
    use crate::types::MegarepoChangeTargetConfig;
    use crate::types::MegarepoRemergeSource;
    use crate::types::MegarepoSyncChangeset;
    macro_rules! test_enqueue_dequeue_and_poll_once {
        {
            $fn_name: ident,
            $request_struct: ident,
            $thrift_params: expr,
            $result: ident,
            $request_type: expr,
        } => {
            #[mononoke::fbinit_test]
            async fn $fn_name(fb: FacebookInit) -> Result<(), Error> {
                println!("Running {}", stringify!($fn_name));
                let ctx = CoreContext::test_mock(fb);
                let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
                let repo_id = repo.repo_identity().id();
                let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();

                // Enqueue a request
                let params = $thrift_params;
                let token = q.enqueue(&ctx, Some(&repo_id), params.clone()).await?;

                // Verify that request metadata is in the db and has expected values
                let row_id = token.to_db_id()?;
                let entry = q
                    .table
                    .test_get_request_entry_by_id(&ctx, &row_id)
                    .await?
                    .expect("Request is missing in the DB");
                assert_eq!(entry.status, RequestStatus::New);
                assert_eq!(entry.started_processing_at, None);
                assert_eq!(entry.ready_at, None);
                assert_eq!(entry.polled_at, None);
                assert_eq!(entry.repo_id, Some(RepositoryId::new(0)));
                assert_eq!(
                    entry.request_type,
                    RequestType($request_type.to_string())
                );
                let req_id = RequestId(row_id, entry.request_type);

                // Verify that poll_once on this request in a "new" state
                // returns None
                let new_poll = q.poll_once::<$request_struct>(&ctx, &req_id).await.map_err(|e| Into::<Error>::into(e))?;
                assert!(new_poll.is_none());

                // Simulate the tailer and grab the element from the queue, this should return the params
                // back and flip its state back to "in_progress"
                let res = q.dequeue(&ctx, &ClaimedBy("tests".to_string())).await;
                let res = match res {
                    Ok(res) => res,
                    Err(err) => panic!("Unexpected error: {:?}", err),
                };
                let res = match res {
                    Some(res) => res,
                    None => panic!("Unexpected None"),
                };
                let req_id = res.id;
                let params_from_store = res.params;

                // Verify that request params from blobstore match what we put there
                assert_eq!(params_from_store, params.into());

                // Verify that request params are in the blobstore

                // Verify that poll_once on this request in a "in_progress" state
                // returns None
                let in_progress_poll = q.poll_once::<$request_struct>(&ctx,  &req_id).await.map_err(|e| Into::<Error>::into(e))?;
                assert!(in_progress_poll.is_none());

                // Inject a result for this request
                // Verify that poll_once on this request in a "in_progress" state
                // returns injected result
                let fake_specific_result: $result = Default::default();
                let fake_result: AsynchronousRequestResult = fake_specific_result.clone().into();
                q.complete(&ctx, &req_id, fake_result).await?;
                let ready_poll = q.poll_once::<$request_struct>(&ctx, &req_id).await.map_err(|e| Into::<Error>::into(e))?;
                let ready_poll_response = ready_poll.unwrap();
                assert_eq!(ready_poll_response, fake_specific_result);

                // After a successful poll, request is marked as polled
                let entry = q.table.test_get_request_entry_by_id(&ctx, &req_id.0).await?.unwrap();
                assert_eq!(entry.status, RequestStatus::Polled);

                Ok(())
            }
        }
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_add_target,
        MegarepoAddSyncTarget,
        ThriftMegarepoAddTargetParams {
            config_with_new_target: ThriftMegarepoSyncTargetConfig {
                target: ThriftMegarepoTarget {
                    bookmark: "oculus".to_string(),
                    repo_id: Some(0),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        MegarepoAddTargetResult,
        "megarepo_add_sync_target",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_add_branching_target,
        MegarepoAddBranchingSyncTarget,
        ThriftMegarepoAddBranchingTargetParams {
            target: ThriftMegarepoTarget {
                bookmark: "oculus".to_string(),
                repo_id: Some(0),
                ..Default::default()
            },
            ..Default::default()
        },
        MegarepoAddBranchingTargetResult,
        "megarepo_add_branching_sync_target",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_sync_changeset,
        MegarepoSyncChangeset,
        ThriftMegarepoSyncChangesetParams {
            target: ThriftMegarepoTarget {
                bookmark: "oculus".to_string(),
                repo: Some(
                    RepoSpecifier {
                        name: "test".to_string(),
                        ..Default::default()
                    }
                ),
                ..Default::default()
            },
            ..Default::default()
        },
        MegarepoSyncChangesetResult,
        "megarepo_sync_changeset",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_change_config,
        MegarepoChangeTargetConfig,
        ThriftMegarepoChangeTargetConfigParams {
            target: ThriftMegarepoTarget {
                bookmark: "oculus".to_string(),
                repo_id: Some(0),
                ..Default::default()
            },
            ..Default::default()
        },
        MegarepoChangeTargetConfigResult,
        "megarepo_change_target_config",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_remerge_source,
        MegarepoRemergeSource,
        ThriftMegarepoRemergeSourceParams {
            target: ThriftMegarepoTarget {
                bookmark: "oculus".to_string(),
                repo_id: Some(0),
                ..Default::default()
            },
            ..Default::default()
        },
        MegarepoRemergeSourceResult,
        "megarepo_remerge_source",
    }

    #[mononoke::fbinit_test]
    async fn test_retry(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        with_just_knobs_async(
            JustKnobsInMemory::new(hashmap! {
                JK_RETRY_LIMIT.to_string() => KnobVal::Int(3),
            }),
            test_retry_impl(&ctx).boxed(),
        )
        .await?;
        Ok(())
    }

    async fn test_retry_impl(ctx: &CoreContext) -> Result<(), Error> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_id = repo.repo_identity().id();
        let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();
        let claimed_by = ClaimedBy("tests".to_string());

        // Enqueue a request
        let params = ThriftMegarepoRemergeSourceParams {
            target: ThriftMegarepoTarget {
                bookmark: "oculus".to_string(),
                repo_id: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
        let token = q.enqueue(ctx, Some(&repo_id), params.clone()).await?;

        // Get the request from the queue
        let row_id = token.to_db_id()?;
        let entry = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id)
            .await?
            .expect("Request is missing in the DB");
        let req_id = RequestId(row_id, entry.request_type);
        assert_eq!(entry.status, RequestStatus::New);
        assert_eq!(entry.num_retries, None);

        // Try "retry" before request is in progress, it should error
        let res = q.retry(ctx, &req_id).await;
        assert!(res.is_err());

        let max_retry_allowed = justknobs::get_as::<u8>(JK_RETRY_LIMIT, Some(&req_id.1.0))?;
        for i in 0..max_retry_allowed {
            // Mark the request as in progress to test retry
            q.table.mark_in_progress(ctx, &req_id, &claimed_by).await?;

            // Retry and verify request metadata values
            let will_retry = q.retry(ctx, &req_id).await?;
            assert!(will_retry);
            let entry = q
                .table
                .test_get_request_entry_by_id(ctx, &row_id)
                .await?
                .expect("Request is missing in the DB");
            assert_eq!(entry.status, RequestStatus::New);
            assert_eq!(entry.num_retries, Some(i + 1));
        }

        // Mark the request as in progress to test retry
        q.table.mark_in_progress(ctx, &req_id, &claimed_by).await?;

        // Now we've used all the retry allowance, next attempt won't be allowed
        let will_retry = q.retry(ctx, &req_id).await?;
        assert!(!will_retry);
        let entry = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id)
            .await?
            .expect("Request is missing in the DB");
        assert_eq!(entry.status, RequestStatus::Failed);
        assert_ne!(entry.failed_at, None);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_concurrency_limit_grouped(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        // derive_boundaries and derive_slice share the "derive_backfill" switch,
        // so they share a single concurrency limit.
        with_just_knobs_async(
            JustKnobsInMemory::new(hashmap! {
                JK_MAX_CONCURRENT.to_string() => KnobVal::Int(3),
            }),
            test_concurrency_limit_grouped_impl(&ctx).boxed(),
        )
        .await?;
        Ok(())
    }

    async fn test_concurrency_limit_grouped_impl(ctx: &CoreContext) -> Result<(), Error> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_id = repo.repo_identity().id();
        let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();
        let claimed_by = ClaimedBy("tests".to_string());

        let make_boundaries_params = || source_control::DeriveBoundariesParams {
            repo_id: repo_id.id() as i64,
            derived_data_type: "fsnodes".to_string(),
            ..Default::default()
        };

        let make_slice_params = || source_control::DeriveSliceParams {
            repo_id: repo_id.id() as i64,
            derived_data_type: "fsnodes".to_string(),
            ..Default::default()
        };

        // Enqueue a mix of derive_boundaries and derive_slice requests
        let token1 = q
            .enqueue(ctx, Some(&repo_id), make_boundaries_params())
            .await?;
        let token2 = q.enqueue(ctx, Some(&repo_id), make_slice_params()).await?;
        let token3 = q
            .enqueue(ctx, Some(&repo_id), make_boundaries_params())
            .await?;
        let token4 = q.enqueue(ctx, Some(&repo_id), make_slice_params()).await?;

        let row_id1 = token1.to_db_id()?;
        let entry1 = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id1)
            .await?
            .expect("Request 1 is missing in the DB");
        let req_id1 = RequestId(row_id1, entry1.request_type);

        let row_id2 = token2.to_db_id()?;
        let entry2 = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id2)
            .await?
            .expect("Request 2 is missing in the DB");
        let req_id2 = RequestId(row_id2, entry2.request_type);

        let row_id3 = token3.to_db_id()?;
        let entry3 = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id3)
            .await?
            .expect("Request 3 is missing in the DB");
        let req_id3 = RequestId(row_id3, entry3.request_type);

        let row_id4 = token4.to_db_id()?;
        let entry4 = q
            .table
            .test_get_request_entry_by_id(ctx, &row_id4)
            .await?
            .expect("Request 4 is missing in the DB");
        let req_id4 = RequestId(row_id4, entry4.request_type);

        let boundaries_type = RequestType("derive_boundaries".to_string());
        let slice_type = RequestType("derive_slice".to_string());

        // With no requests in progress, both types should pass
        assert!(!q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(!q.concurrency_limit_reached(ctx, &slice_type).await?);

        // Mark first request (derive_boundaries) as in-progress
        q.table.mark_in_progress(ctx, &req_id1, &claimed_by).await?;

        // 1 in group, limit 3 — should pass for both types
        assert!(!q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(!q.concurrency_limit_reached(ctx, &slice_type).await?);

        // Mark second request (derive_slice) as in-progress
        q.table.mark_in_progress(ctx, &req_id2, &claimed_by).await?;

        // 2 in group (1 boundaries + 1 slice), limit 3 — should still pass
        assert!(!q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(!q.concurrency_limit_reached(ctx, &slice_type).await?);

        // Mark third request (derive_boundaries) as in-progress
        q.table.mark_in_progress(ctx, &req_id3, &claimed_by).await?;

        // 3 in group (2 boundaries + 1 slice), at limit 3 — not exceeded yet
        assert!(!q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(!q.concurrency_limit_reached(ctx, &slice_type).await?);

        // Mark fourth request (derive_slice) as in-progress
        q.table.mark_in_progress(ctx, &req_id4, &claimed_by).await?;

        // 4 in group, limit 3 — exceeded
        assert!(q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(q.concurrency_limit_reached(ctx, &slice_type).await?);

        // Complete the first request — back to 3 in group, at limit but not over
        let fake_result: AsynchronousRequestResult =
            source_control::DeriveBoundariesResponse::default().into();
        q.complete(ctx, &req_id1, fake_result).await?;
        assert!(!q.concurrency_limit_reached(ctx, &boundaries_type).await?);
        assert!(!q.concurrency_limit_reached(ctx, &slice_type).await?);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_concurrency_limit_ungrouped(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        // Ungrouped types use their own name as the switch and only count
        // their own in-progress requests.
        with_just_knobs_async(
            JustKnobsInMemory::new(hashmap! {
                JK_MAX_CONCURRENT.to_string() => KnobVal::Int(1),
            }),
            async {
                let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
                let repo_id = repo.repo_identity().id();
                let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();
                let claimed_by = ClaimedBy("tests".to_string());

                let megarepo_type = RequestType("megarepo_sync_changeset".to_string());

                // No in-progress requests — should pass
                assert!(!q.concurrency_limit_reached(&ctx, &megarepo_type).await?);

                // Enqueue and mark a megarepo request in-progress
                let params = ThriftMegarepoSyncChangesetParams {
                    target: ThriftMegarepoTarget {
                        bookmark: "oculus".to_string(),
                        repo: Some(RepoSpecifier {
                            name: "test".to_string(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let token = q.enqueue(&ctx, Some(&repo_id), params.clone()).await?;
                let row_id = token.to_db_id()?;
                let entry = q
                    .table
                    .test_get_request_entry_by_id(&ctx, &row_id)
                    .await?
                    .expect("Request is missing in the DB");
                let req_id = RequestId(row_id, entry.request_type);
                q.table.mark_in_progress(&ctx, &req_id, &claimed_by).await?;

                // 1 in-progress, at limit 1 — not exceeded yet
                assert!(!q.concurrency_limit_reached(&ctx, &megarepo_type).await?);

                // Enqueue and mark a second request in-progress
                let token2 = q.enqueue(&ctx, Some(&repo_id), params).await?;
                let row_id2 = token2.to_db_id()?;
                let entry2 = q
                    .table
                    .test_get_request_entry_by_id(&ctx, &row_id2)
                    .await?
                    .expect("Request 2 is missing in the DB");
                let req_id2 = RequestId(row_id2, entry2.request_type);
                q.table
                    .mark_in_progress(&ctx, &req_id2, &claimed_by)
                    .await?;

                // 2 in-progress, limit 1 — exceeded
                assert!(q.concurrency_limit_reached(&ctx, &megarepo_type).await?);

                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_concurrency_limit_zero_means_unlimited(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        with_just_knobs_async(
            JustKnobsInMemory::new(hashmap! {
                JK_MAX_CONCURRENT.to_string() => KnobVal::Int(0),
            }),
            async {
                let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
                let repo_id = repo.repo_identity().id();
                let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();
                let request_type = RequestType("derive_boundaries".to_string());

                // Zero means unlimited — should always pass
                assert!(!q.concurrency_limit_reached(&ctx, &request_type).await?);
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_get_and_fail_requests_by_root_id(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_id = repo.repo_identity().id();
        let q = AsyncMethodRequestQueue::new_test_in_memory(Some(vec![repo_id])).unwrap();
        let claimed_by = ClaimedBy("test_worker".to_string());

        // Use a fixed root_request_id for all test requests
        let root_row_id = RowId(999999);

        // Create three child requests with the same root_request_id
        let child1_params = ThriftMegarepoSyncChangesetParams {
            target: ThriftMegarepoTarget {
                bookmark: "child1".to_string(),
                repo: Some(RepoSpecifier {
                    name: "test".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let child1_token = q
            .enqueue_with_root(&ctx, Some(&repo_id), child1_params, &root_row_id)
            .await?;
        let child1_row_id = child1_token.to_db_id()?;

        let child2_params = ThriftMegarepoSyncChangesetParams {
            target: ThriftMegarepoTarget {
                bookmark: "child2".to_string(),
                repo: Some(RepoSpecifier {
                    name: "test".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let child2_token = q
            .enqueue_with_root(&ctx, Some(&repo_id), child2_params, &root_row_id)
            .await?;
        let child2_row_id = child2_token.to_db_id()?;

        let child3_params = ThriftMegarepoSyncChangesetParams {
            target: ThriftMegarepoTarget {
                bookmark: "child3".to_string(),
                repo: Some(RepoSpecifier {
                    name: "test".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let child3_token = q
            .enqueue_with_root(&ctx, Some(&repo_id), child3_params, &root_row_id)
            .await?;
        let child3_row_id = child3_token.to_db_id()?;

        // Mark child2 as in-progress
        let child2_entry = q
            .table
            .test_get_request_entry_by_id(&ctx, &child2_row_id)
            .await?
            .expect("Child2 request missing");
        let child2_req_id = RequestId(child2_row_id.clone(), child2_entry.request_type.clone());
        q.table
            .mark_in_progress(&ctx, &child2_req_id, &claimed_by)
            .await?;

        // Test get_requests_by_root_id - should return all 3 children
        let requests = q.get_requests_by_root_id(&ctx, &root_row_id).await?;
        assert_eq!(
            requests.len(),
            3,
            "Should find 3 requests with the same root_request_id"
        );

        // Verify the requests are the ones we created
        let request_ids: Vec<_> = requests.iter().map(|(req_id, _, _, _)| &req_id.0).collect();
        assert!(
            request_ids.contains(&&child1_row_id),
            "Should include child1 request"
        );
        assert!(
            request_ids.contains(&&child2_row_id),
            "Should include child2 request"
        );
        assert!(
            request_ids.contains(&&child3_row_id),
            "Should include child3 request"
        );

        // Test fail_new_requests_by_root_id - should fail only 'new' requests
        let failed_count = q.fail_new_requests_by_root_id(&ctx, &root_row_id).await?;
        assert_eq!(
            failed_count, 2,
            "Should fail 2 'new' requests (child1 and child3)"
        );

        // Verify child1 and child3 are now failed
        let child1_entry = q
            .table
            .test_get_request_entry_by_id(&ctx, &child1_row_id)
            .await?
            .expect("Child1 request missing");
        assert_eq!(child1_entry.status, RequestStatus::Failed);

        let child3_entry = q
            .table
            .test_get_request_entry_by_id(&ctx, &child3_row_id)
            .await?
            .expect("Child3 request missing");
        assert_eq!(child3_entry.status, RequestStatus::Failed);

        // Verify child2 is still in-progress (not affected by fail_new_requests_by_root_id)
        let child2_entry_after = q
            .table
            .test_get_request_entry_by_id(&ctx, &child2_row_id)
            .await?
            .expect("Child2 request missing");
        assert_eq!(child2_entry_after.status, RequestStatus::InProgress);

        Ok(())
    }
}
