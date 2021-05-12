/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;

mod store;
mod types;

pub use crate::store::SqlLongRunningRequestsQueue;
pub use crate::types::{
    BlobstoreKey, LongRunningRequestEntry, RequestId, RequestStatus, RequestType, RowId,
};

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
    /// Schedule an execution of a request, given
    /// the request type and the blobstore key of
    /// serialized request parameters
    async fn add_request(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        args_blobstore_key: &BlobstoreKey,
    ) -> Result<RowId>;

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
    async fn mark_in_progress(&self, ctx: &CoreContext, req_id: &RequestId) -> Result<bool>;

    /// Mark request as ready
    async fn mark_ready(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        blobstore_result_key: BlobstoreKey,
    ) -> Result<bool>;

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
}
