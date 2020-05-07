/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

use anyhow::Result;
use async_trait::async_trait;
use limits::types::RateLimits;
pub use session_id::SessionId;
use std::{fmt, sync::Arc, time::Duration};

pub type ArcLoadLimiter = Arc<dyn LoadLimiter + Send + Sync + 'static>;
pub type BoxLoadLimiter = Box<dyn LoadLimiter + Send + Sync + 'static>;

pub type LoadCost = f64;

#[derive(Debug)]
pub enum Metric {
    EgressBytes,
    IngressBlobstoreBytes,
    EgressTotalManifests,
    EgressGetfilesFiles,
    EgressGetpackFiles,
    EgressCommits,
}

#[async_trait]
pub trait LoadLimiter: fmt::Debug {
    async fn should_throttle(&self, metric: Metric, window: Duration) -> Result<bool>;

    fn bump_load(&self, metric: Metric, load: LoadCost);

    fn category(&self) -> &str;

    fn rate_limits(&self) -> &RateLimits;
}

pub struct LoadLimiterBuilder {}
