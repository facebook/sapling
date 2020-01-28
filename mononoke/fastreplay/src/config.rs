/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use serde::Deserialize;

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
            },
        }
    }
}

impl FastReplayConfig {
    pub fn admission_rate(&self) -> i64 {
        self.inner.admission_rate
    }
}
