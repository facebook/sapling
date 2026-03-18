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
pub use crate::types::RequestId;
pub use crate::types::RequestStatus;
pub use crate::types::RequestType;
pub use crate::types::RowId;

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
    ) -> Result<RowId>;

    /// Claim one of new requests. Mark it as in-progress and return it.
    async fn claim_and_get_new_request(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
        supported_repos: Option<&[RepositoryId]>,
    ) -> Result<Option<LongRunningRequestEntry>>;

    /// Get the full request object entry by id
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
    async fn find_abandoned_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
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

    /// List all requests, optionally filtered by repo_id and/or date.
    async fn list_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<Vec<LongRunningRequestEntry>>;

    /// Retrieve stats on the queue, optionally filtered by repo_id.
    /// If `exclude_backfill` is true, derived data backfill request types
    /// (derive_boundaries, derive_slice, derive_backfill, derive_backfill_repo)
    /// are excluded from the stats.
    async fn get_queue_stats(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
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
    /// to the top-level request that spawned it.
    async fn add_request_with_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        root_request_id: &RowId,
    ) -> Result<RowId>;

    /// Add a request with dependencies and a root_request_id.
    async fn add_request_with_dependencies_and_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        depends_on: &[RowId],
        root_request_id: &RowId,
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

    /// List recent backfill jobs with repo counts.
    async fn list_recent_backfills_with_repo_count(
        &self,
        ctx: &CoreContext,
        min_created_at: &Timestamp,
    ) -> Result<Vec<(RowId, Timestamp, RequestStatus, i64)>>;

    /// Get the root backfill entry by ID.
    async fn get_backfill_root_entry(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<(RowId, RequestType, RequestStatus, Timestamp, BlobstoreKey)>>;
}
