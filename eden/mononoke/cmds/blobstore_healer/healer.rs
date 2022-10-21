/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::Sum;
use std::ops::Add;
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

#[derive(Default, Debug, PartialEq)]
pub(crate) struct HealStats {
    pub queue_add: usize,
    pub queue_del: usize,
    pub put_success: usize,
    pub put_failure: usize,
}

impl Add for HealStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            queue_add: self.queue_add + other.queue_add,
            queue_del: self.queue_del + other.queue_del,
            put_success: self.put_success + other.put_success,
            put_failure: self.put_failure + other.put_failure,
        }
    }
}

impl Sum for HealStats {
    fn sum<I: Iterator<Item = HealStats>>(iter: I) -> HealStats {
        iter.fold(Default::default(), Add::add)
    }
}
