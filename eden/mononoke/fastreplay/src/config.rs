/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::num::NonZeroU64;

use fastreplay_structs::FastReplayConfig as RawFastReplayConfig;

/// Wraps RawFastReplayConfig into a FastReplayConfig in order to provide our own Default
/// implementation. This lets us provide a default 100% admission rate instead of depending on the
/// default from Thrift.
#[derive(Deserialize)]
pub struct FastReplayConfig {
    #[serde(flatten)]
    inner: RawFastReplayConfig,
}

impl Default for FastReplayConfig {
    fn default() -> Self {
        FastReplayConfig {
            inner: RawFastReplayConfig {
                admission_rate: 100,
                max_concurrency: 50,
                scuba_sampling_target: 1,
                skipped_repos: BTreeSet::default(),
            },
        }
    }
}

impl FastReplayConfig {
    pub fn admission_rate(&self) -> i64 {
        self.inner.admission_rate
    }

    pub fn max_concurrency(&self) -> Result<NonZeroU64, Error> {
        // NOTE: The config comes as an i64. It should be > 0 since we validate that, but let's be
        // safe if not.
        NonZeroU64::new(u64::try_from(self.inner.max_concurrency)?)
            .ok_or_else(|| Error::msg("invalid scuba_sampling_target"))
            .with_context(|| {
                format!(
                    "While converting {:?} to max_concurrency",
                    self.inner.max_concurrency
                )
            })
    }

    pub fn scuba_sampling_target(&self) -> Result<NonZeroU64, Error> {
        // NOTE: The config comes as an i64. Same as above.
        NonZeroU64::new(u64::try_from(self.inner.scuba_sampling_target)?)
            .ok_or_else(|| Error::msg("invalid scuba_sampling_target"))
            .with_context(|| {
                format!(
                    "While converting {:?} to scuba_sampling_target",
                    self.inner.scuba_sampling_target
                )
            })
    }

    pub fn skipped_repos(&self) -> &BTreeSet<String> {
        &self.inner.skipped_repos
    }
}
