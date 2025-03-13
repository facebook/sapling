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

use crate::client::EdenFsClient;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigValue {
    pub parsed_value: String,
    pub source_type: thrift_types::edenfs_config::ConfigSourceType,
    pub source_path: Vec<u8>,
}

impl From<thrift_types::edenfs_config::ConfigValue> for ConfigValue {
    fn from(from: thrift_types::edenfs_config::ConfigValue) -> Self {
        Self {
            parsed_value: from.parsedValue,
            source_type: from.sourceType,
            source_path: from.sourcePath,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigData {
    pub values: BTreeMap<String, ConfigValue>,
}

impl From<thrift_types::edenfs_config::EdenConfigData> for ConfigData {
    fn from(from: thrift_types::edenfs_config::EdenConfigData) -> Self {
        Self {
            values: from
                .values
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect::<BTreeMap<_, _>>(),
        }
    }
}

impl<'a> EdenFsClient<'a> {
    pub async fn get_config_default(&self) -> Result<ConfigData> {
        let params: thrift_types::edenfs::GetConfigParams = Default::default();
        self.with_client(|client| client.getConfig(&params))
            .await
            .with_context(|| "failed to get default eden config data")
            .map(|config_data| config_data.into())
            .map_err(EdenFsError::from)
    }
}
