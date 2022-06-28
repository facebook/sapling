/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::RepoShardedProcess;
use crate::RepoShardedProcessExecutor;
use anyhow::Result;
use fbinit::FacebookInit;
use slog::Logger;
use std::sync::Arc;
use tokio::runtime::Handle;

pub struct BackgroundProcessExecutor {}

impl BackgroundProcessExecutor {
    pub fn new(
        _fb: FacebookInit,
        _runtime_handle: Handle,
        _logger: &Logger,
        _service_name: &'static str,
        _service_scope: &'static str,
        _timeout_secs: u64,
        _bp_handle: Arc<dyn RepoShardedProcess>,
    ) -> Result<Self> {
        unimplemented!("BackgroundProcessExecutor is supported only for fbcode build")
    }

    pub async fn block_and_execute(&mut self, logger: &Logger) -> Result<()> {
        unimplemented!("BackgroundProcessExecutor is supported only for fbcode build")
    }
}
