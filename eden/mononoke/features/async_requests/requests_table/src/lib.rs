/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![recursion_limit = "256"]

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;

mod store;
mod types;

pub use crate::store::SqlLongRunningRequestsQueue;
pub use crate::types::BlobstoreKey;
pub use crate::types::ClaimedBy;
pub use crate::types::LongRunningRequestEntry;
pub use crate::types::QueueStats;
pub use crate::types::QueueStatsEntry;
pub use crate::types::RecentBackfillEntry;
pub use crate::types::RequestId;
pub use crate::types::RequestStatus;
pub use crate::types::RequestType;
pub use crate::types::RowId;

/// Controls which repos a queue operation applies to.
#[derive(Clone, Debug)]
pub enum QueueRepoFilter {
    /// Only operate on requests for these specific repos.
    Only(Vec<RepositoryId>),
    /// Operate on requests for any repo except these.
    Except(Vec<RepositoryId>),
}

/// Controls which request types a queue operation applies to.
#[derive(Clone, Debug)]
pub enum QueueRequestTypeFilter {
    /// Accept all request types (no filtering).
    All,
    /// Only accept requests of these types.
    Only(Vec<RequestType>),
    /// Accept all request types except these.
    Except(Vec<RequestType>),
}

impl QueueRequestTypeFilter {
    /// Resolve this filter into an explicit include-list of `RequestType`
    /// values for use in SQL `IN (...)` clauses.
    pub fn resolve_to_include_list(&self) -> Vec<RequestType> {
        match self {
            QueueRequestTypeFilter::All => async_requests_types::ALL_REQUEST_TYPE_NAMES
                .iter()
                .map(|s| RequestType(s.to_string()))
                .collect(),
            QueueRequestTypeFilter::Only(types) => types.clone(),
            QueueRequestTypeFilter::Except(excluded) => {
                async_requests_types::ALL_REQUEST_TYPE_NAMES
                    .iter()
                    .filter(|s| !excluded.iter().any(|ex| ex.0 == **s))
                    .map(|s| RequestType(s.to_string()))
                    .collect()
            }
        }
    }
}

/// A queue of long-running requests
/// This is designed to support the use case of
/// asynchronous request processing, when a client
/// service schedules a request to be processed
/// and later checks if the result is ready.
/// Another service handles the processing and
/// state updates for individual requests.
#[facet::facet]
#[async_trait]
pub trait LongRunningRequestsQueue: Send + Sync {
    /// Schedule an execution of a request, given the request type, the blobstore
    /// key of serialized request parameters and (repo, bookmark) pair
    /// representing the target of the request.
    async fn add_request(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        created_by: Option<&str>,
    ) -> Result<RowId>;

    /// Claim one of new requests. Mark it as in-progress and return it.
    /// `repo_filter` controls which repos are eligible for dequeuing.
    /// `request_type_filter` controls which request types are eligible.
    async fn claim_and_get_new_request(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
        repo_filter: &QueueRepoFilter,
        request_type_filter: &QueueRequestTypeFilter,
    ) -> Result<Option<LongRunningRequestEntry>>;

    /// Get the full request object entry by id.
    ///
    /// This does not take `request_type`, so callers should only use it for
    /// diagnostics or cases where the type is expected to be read from the row.
    async fn get_request_entry_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<LongRunningRequestEntry>>;

    /// Get the full request object entry by id.
    /// Since this does not take `request_type`, it is
    /// mainly intended to be used in tests (`request_type`
    /// is a type-safety feature)
    async fn test_get_request_entry_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<LongRunningRequestEntry>>;

    /// Mark request as in-progress
    async fn mark_in_progress(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        claimed_by: &ClaimedBy,
    ) -> Result<bool>;

    /// Update the inprogress_last_updated_at timestamp
    /// This is used to mark that request is still executing
    async fn update_in_progress_timestamp(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool>;

    /// Find requests that have "inprogress" status but which timestamp
    /// hasn't been updated after `abandoned_timestamp`.
    /// `request_type_filter` controls which request types are eligible.
    async fn find_abandoned_requests(
        &self,
        ctx: &CoreContext,
        repo_filter: &QueueRepoFilter,
        request_type_filter: &QueueRequestTypeFilter,
        abandoned_timestamp: Timestamp,
    ) -> Result<Vec<RequestId>>;

    /// If `request_id` is still abandoned, then mark it as new so that
    /// somebody else can pick it up
    async fn mark_abandoned_request_as_new(
        &self,
        ctx: &CoreContext,
        request_id: RequestId,
        abandoned_timestamp: Timestamp,
    ) -> Result<bool>;

    /// Mark request as ready
    async fn mark_ready(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        blobstore_result_key: BlobstoreKey,
    ) -> Result<bool>;

    /// Mark request as new (used for requeuing requests from CLI)
    async fn mark_new(&self, ctx: &CoreContext, req_id: &RequestId) -> Result<bool>;

    /// Mark request as polled by a client
    /// To be used in tests only
    async fn test_mark(
        &self,
        ctx: &CoreContext,
        row_id: &RowId,
        status: RequestStatus,
    ) -> Result<bool>;

    /// Query request and change it's state to `polled`
    /// if it is ready. This fn will return a corresponding
    /// LongRunningRequestEntry if the request is in `ready`
    /// or `polled` state at the time of the call
    /// Otherwise, it will return None. It will error out
    /// for unknown requests
    async fn poll(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<(bool, LongRunningRequestEntry)>>;

    /// List all requests, filtered by repo and/or date.
    async fn list_requests(
        &self,
        ctx: &CoreContext,
        repo_filter: &QueueRepoFilter,
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<Vec<LongRunningRequestEntry>>;

    /// List one page of requests in the `ready` state with `id` strictly
    /// greater than `after_id`, ordered by `id` ascending, up to `limit` rows.
    ///
    /// This is keyset pagination over the primary key: callers page through
    /// all `ready` requests by passing the largest `id` from the previous page
    /// as `after_id` (starting from `RowId(0)`). It is intended for scans such
    /// as orphan detection, where a single bounded query per batch keeps the
    /// load on the DB predictable. Only `ready` requests are returned because
    /// those are the ones whose params blob is expected to still exist and
    /// worth checking; in-flight (`new`/`inprogress`) requests are skipped.
    async fn list_ready_requests_after_id(
        &self,
        ctx: &CoreContext,
        after_id: &RowId,
        limit: usize,
    ) -> Result<Vec<LongRunningRequestEntry>>;

    /// Mark the given requests as `failed`. Only rows still in the `ready`
    /// state are affected; the guard keeps this safe to run concurrently with
    /// other queue activity and idempotent on re-runs. Returns the number of
    /// rows actually updated.
    async fn mark_ready_requests_failed(&self, ctx: &CoreContext, ids: &[RowId]) -> Result<u64>;

    /// Retrieve stats on the queue, filtered by repo.
    /// If `exclude_backfill` is true, derived data backfill request types
    /// (derive_boundaries, derive_slice, derive_backfill, derive_backfill_repo)
    /// are excluded from the stats.
    async fn get_queue_stats(
        &self,
        ctx: &CoreContext,
        repo_filter: &QueueRepoFilter,
        exclude_backfill: bool,
    ) -> Result<QueueStats>;

    /// Query how many times the request has been retried.
    /// If it's within the retry allowance, bump the retry count,
    /// set the status to `new` and return true,
    /// otherwise, set the status to `failed` and return false.
    async fn update_for_retry_or_fail(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        max_retry_allowed: u8,
    ) -> Result<bool>;

    /// Add a request with optional dependencies.
    /// A request will remain in 'new' status until ALL of its dependencies reach 'ready' or 'polled' status.
    async fn add_request_with_dependencies(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        depends_on: &[RowId],
        created_by: Option<&str>,
    ) -> Result<RowId>;

    /// Get all dependency request IDs for a given request.
    /// Returns the IDs of requests that must complete before this request becomes eligible for execution.
    async fn get_dependencies(&self, ctx: &CoreContext, request_id: &RowId) -> Result<Vec<RowId>>;

    /// Mark a request as failed and cascade the failure to all dependent requests.
    /// First marks all dependent requests as failed, then marks the specified request as failed.
    async fn mark_failed_with_cascade(&self, ctx: &CoreContext, req_id: &RowId) -> Result<bool>;

    /// Count in-progress requests for the given request types
    async fn count_inprogress_by_types(
        &self,
        ctx: &CoreContext,
        request_types: &[&str],
    ) -> Result<i64>;

    /// Schedule an execution of a request with a root_request_id linking it
    /// to the top-level request that spawned it. For repo-scoped requests, if
    /// the same child request was already enqueued, return the existing row id.
    async fn add_request_with_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        root_request_id: &RowId,
        created_by: Option<&str>,
    ) -> Result<RowId>;

    /// Add a request with dependencies and a root_request_id. For repo-scoped
    /// requests, if the same child request was already enqueued, return the
    /// existing row id and add dependencies idempotently.
    async fn add_request_with_dependencies_and_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        depends_on: &[RowId],
        root_request_id: &RowId,
        created_by: Option<&str>,
    ) -> Result<RowId>;

    /// Get all requests that share a given root_request_id.
    async fn get_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<Vec<LongRunningRequestEntry>>;

    /// Mark all 'new' requests with the given root_request_id as 'failed'.
    /// Returns the number of requests affected.
    async fn fail_new_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<u64>;

    /// Get aggregated statistics by request type and status for a backfill,
    /// optionally filtered to a specific repo.
    async fn get_backfill_stats(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
        repo_id: Option<&RepositoryId>,
    ) -> Result<Vec<(RequestType, RequestStatus, i64)>>;

    /// Get aggregated statistics by repo and status for a backfill.
    async fn get_backfill_stats_by_repo(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<Vec<(Option<RepositoryId>, RequestStatus, i64)>>;

    /// Get timing statistics for a backfill (completed count, avg duration, date range).
    async fn get_backfill_timing_stats(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<(i64, Option<f64>, Option<Timestamp>, Option<Timestamp>)>;

    /// List recent backfill jobs with repo counts and aggregated child status counts.
    async fn list_recent_backfills_with_repo_count(
        &self,
        ctx: &CoreContext,
        min_created_at: &Timestamp,
    ) -> Result<Vec<RecentBackfillEntry>>;

    /// Get the root backfill entry by ID.
    async fn get_backfill_root_entry(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<
        Option<(
            RowId,
            RequestType,
            RequestStatus,
            Timestamp,
            BlobstoreKey,
            Option<String>,
        )>,
    >;
}
