/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;

mod store;
mod types;

pub use crate::store::SqlLongRunningRequestsQueue;
pub use crate::types::BlobstoreKey;
pub use crate::types::ClaimedBy;
pub use crate::types::LongRunningRequestEntry;
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
        repo_id: &RepositoryId,
        bookmark: &BookmarkName,
        args_blobstore_key: &BlobstoreKey,
    ) -> Result<RowId>;

    /// Claim one of new requests. Mark it as in-progress and return it.
    async fn claim_and_get_new_request(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
        supported_repos: &[RepositoryId],
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
        repo_ids: &[RepositoryId],
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

    async fn list_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: &[RepositoryId],
        statuses: &[RequestStatus],
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<Vec<LongRunningRequestEntry>>;
}
