/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::path_from_bytes;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ConfigSourceType {
    Default = 0,
    SystemConfig = 1,
    UserConfig = 2,
    CommandLine = 3,
    Dynamic = 4,
    Undefined = -1,
}

impl From<thrift_types::edenfs_config::ConfigSourceType> for ConfigSourceType {
    fn from(from: thrift_types::edenfs_config::ConfigSourceType) -> Self {
        match from {
            thrift_types::edenfs_config::ConfigSourceType::Default => Self::Default,
            thrift_types::edenfs_config::ConfigSourceType::SystemConfig => Self::SystemConfig,
            thrift_types::edenfs_config::ConfigSourceType::UserConfig => Self::UserConfig,
            thrift_types::edenfs_config::ConfigSourceType::CommandLine => Self::CommandLine,
            thrift_types::edenfs_config::ConfigSourceType::Dynamic => Self::Dynamic,
            _ => Self::Undefined,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigValue {
    pub parsed_value: String,
    pub source_type: ConfigSourceType,
    pub source_path: PathBuf,
}

impl From<thrift_types::edenfs_config::ConfigValue> for ConfigValue {
    fn from(from: thrift_types::edenfs_config::ConfigValue) -> Self {
        Self {
            parsed_value: from.parsedValue,
            source_type: from.sourceType.into(),
            source_path: path_from_bytes(&from.sourcePath)
                .expect("Failed to convert ConfigValue::sourcePath."),
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

impl EdenFsClient {
    pub async fn get_config_default(&self) -> Result<ConfigData> {
        let params: thrift_types::edenfs::GetConfigParams = Default::default();
        self.with_thrift(|thrift| (thrift.getConfig(&params), EdenThriftMethod::GetConfig))
            .await
            .with_context(|| "failed to get default eden config data")
            .map(|config_data| config_data.into())
            .map_err(EdenFsError::from)
    }
}
