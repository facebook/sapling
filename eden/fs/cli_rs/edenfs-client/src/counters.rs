/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;

use crate::client::Client;
use crate::client::EdenFsClient;

impl EdenFsClient {
    pub async fn get_regex_counters(&self, arg_regex: &str) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| thrift.getRegexCounters(arg_regex))
            .await
            .with_context(|| "failed to get regex counters")
            .map_err(EdenFsError::from)
    }

    pub async fn get_selected_counters(&self, keys: &[String]) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| thrift.getSelectedCounters(keys))
            .await
            .with_context(|| "failed to get selected counters")
            .map_err(EdenFsError::from)
    }

    pub async fn get_counters(&self) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| thrift.getCounters())
            .await
            .with_context(|| "failed to get counters")
            .map_err(EdenFsError::from)
    }

    pub async fn get_counter(&self, key: &str) -> Result<i64> {
        self.with_thrift(|thrift| thrift.getCounter(key))
            .await
            .with_context(|| format!("failed to get counter for key {}", key))
            .map_err(EdenFsError::from)
    }
}
