/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use fbinit::FacebookInit;
use slog::Logger;
use tokio::runtime::Handle;

use crate::RepoShardedProcess;

pub struct ShardedProcessExecutor {}

impl ShardedProcessExecutor {
    pub fn new(
        _fb: FacebookInit,
        _runtime_handle: Handle,
        _logger: &Logger,
        _service_name: &'static str,
        _service_scope: &'static str,
        _timeout_secs: u64,
        _bp_handle: Arc<dyn RepoShardedProcess>,
        _shard_healing: bool,
    ) -> Result<Self> {
        unimplemented!("ShardedProcessExecutor is supported only for fbcode build")
    }

    pub async fn block_and_execute(
        &mut self,
        _logger: &Logger,
        _terminate_process: Arc<AtomicBool>,
    ) -> Result<()> {
        unimplemented!("ShardedProcessExecutor is supported only for fbcode build")
    }

    pub fn execute(&mut self, _logger: &Logger) {
        unimplemented!("ShardedProcessExecutor is supported only for fbcode build")
    }
}
