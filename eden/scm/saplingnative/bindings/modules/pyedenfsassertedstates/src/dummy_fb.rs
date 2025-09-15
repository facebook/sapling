/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::time::Duration;

pub struct ContentLockGuard {}
pub struct AssertedStatesClient {}

impl AssertedStatesClient {
    pub fn new(_path: &PathBuf) -> anyhow::Result<Self> {
        let _ = _path;
        Ok(Self {})
    }

    pub fn enter_state_with_deadline(
        &self,
        _state: &str,
        _deadline: Duration,
        _backoff: Duration,
    ) -> anyhow::Result<ContentLockGuard> {
        Ok(ContentLockGuard {})
    }
}
