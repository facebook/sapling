/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;

use crate::client::EdenFsClient;

impl EdenFsClient {
    pub async fn flush_stats_now(&self) -> Result<()> {
        self.with_thrift(|thrift| thrift.flushStatsNow())
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to call flushstatsNow")))
    }
}
