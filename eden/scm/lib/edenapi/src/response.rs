/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::prelude::*;

use async_runtime::block_on;
use http_client::Stats;

use crate::errors::EdenApiError;

pub use edenapi_trait::Fetch;
pub use edenapi_trait::ResponseMeta;

/// Non-async version of `Fetch`.
pub struct BlockingFetch<T> {
    pub meta: Vec<ResponseMeta>,
    pub entries: Vec<T>,
    pub stats: Stats,
}

impl<T> BlockingFetch<T> {
    pub(crate) fn from_async<F>(fetch: F) -> Result<Self, EdenApiError>
    where
        F: Future<Output = Result<Fetch<T>, EdenApiError>>,
    {
        let Fetch {
            meta,
            entries,
            stats,
        } = block_on(fetch)?;

        let entries = block_on(entries.try_collect())?;
        let stats = block_on(stats)?;

        Ok(Self {
            meta,
            entries,
            stats,
        })
    }
}
