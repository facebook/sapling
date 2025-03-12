/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;

use crate::client::EdenFsClient;

impl<'a> EdenFsClient<'a> {
    pub async fn get_regex_counters(&self, arg_regex: &str) -> Result<BTreeMap<String, i64>> {
        self.client
            .getRegexCounters(arg_regex)
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get regex counters")))
    }
}
