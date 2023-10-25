/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use async_runtime::block_unless_interrupted;
use clientinfo::get_client_request_info_thread_local;
use clientinfo_async::with_client_request_info_scope;
pub use edenapi_trait::Response;
pub use edenapi_trait::ResponseMeta;
use futures::prelude::*;
use http_client::Stats;

use crate::errors::EdenApiError;

/// Non-async version of `Response`.
pub struct BlockingResponse<T> {
    pub entries: Vec<T>,
    pub stats: Stats,
}

impl<T> BlockingResponse<T> {
    pub fn from_async<F>(fetch: F) -> Result<Self, EdenApiError>
    where
        F: Future<Output = Result<Response<T>, EdenApiError>>,
    {
        // Fetch ClientRequestInfo from a thread local and pass to async code
        let maybe_client_request_info = get_client_request_info_thread_local();
        let Response { entries, stats } = block_unless_interrupted(with_client_request_info_scope(
            maybe_client_request_info.clone(),
            fetch,
        ))
        .context("transfer interrupted by user")??;
        let entries = block_unless_interrupted(with_client_request_info_scope(
            maybe_client_request_info.clone(),
            entries.try_collect(),
        ))
        .context("transfer interrupted by user")??;
        let stats = block_unless_interrupted(with_client_request_info_scope(
            maybe_client_request_info,
            stats,
        ))
        .context("transfer interrupted by user")??;
        Ok(Self { entries, stats })
    }
}
