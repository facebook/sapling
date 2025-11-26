/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use context::CoreContext;

pub(crate) const MIN_FETCH_FAILURE_DELAY: Duration = Duration::from_millis(1);
pub(crate) const MAX_FETCH_FAILURE_DELAY: Duration = Duration::from_millis(100);
pub(crate) const DEFAULT_BLOB_SIZE_BYTES: u64 = 1024;

#[derive(Clone, Debug)]
pub struct HealResult {
    pub processed_full_batch: bool,
    pub processed_rows: u64,
}

#[async_trait]
pub trait Healer {
    async fn heal(&self, ctx: &CoreContext, minimum_age: ChronoDuration) -> Result<HealResult>;
}
