/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::RwLock;

use anyhow::Error;
use anyhow::anyhow;
use observability_config::ObservabilityConfig as CfgrObservabilityConfig;
use observability_config::ScubaObservabilityConfig as CfgrScubaObservabilityConfig;
use observability_config::ScubaVerbosityLevel as CfgrScubaVerbosityLevel;
use regex::Regex;
use serde::Deserialize;
use serde::de::Deserializer;
use serde::de::Error as _;

fn cfgr_to_scuba_level(level: &CfgrScubaVerbosityLevel) -> Result<ScubaVerbosityLevel, Error> {
    match *level {
        CfgrScubaVerbosityLevel::Normal => Ok(ScubaVerbosityLevel::Normal),
        CfgrScubaVerbosityLevel::Verbose => Ok(ScubaVerbosityLevel::Verbose),
        other => Err(anyhow!("unexpected ScubaLoggingLevel: {:?}", other)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScubaVerbosityLevel {
    Normal,
    Verbose,
}

pub struct ScubaObservabilityConfig {
    pub level: ScubaVerbosityLevel,
    pub verbose_sessions: Vec<String>,
    pub verbose_unixnames: Vec<String>,
    pub verbose_source_hostname_regexes: RwLock<Vec<Regex>>,
}

pub struct ObservabilityConfig {
    pub scuba_config: ScubaObservabilityConfig,
}

impl TryFrom<CfgrScubaObservabilityConfig> for ScubaObservabilityConfig {
    type Error = Error;

    fn try_from(value: CfgrScubaObservabilityConfig) -> Result<Self, Error> {
        let CfgrScubaObservabilityConfig {
            level,
            verbose_sessions,
            verbose_unixnames,
            verbose_source_hostnames,
            ..
        } = value;
        let regexes = verbose_source_hostnames
            .into_iter()
            .map(|s| Regex::new(&s))
            .collect::<Result<Vec<Regex>, _>>()?;
        Ok(Self {
            level: cfgr_to_scuba_level(&level)?,
            verbose_sessions,
            verbose_unixnames,
            verbose_source_hostname_regexes: RwLock::new(regexes),
        })
    }
}

impl TryFrom<CfgrObservabilityConfig> for ObservabilityConfig {
    type Error = Error;

    fn try_from(value: CfgrObservabilityConfig) -> Result<Self, Error> {
        let CfgrObservabilityConfig { scuba_config, .. } = value;

        Ok(Self {
            scuba_config: scuba_config.try_into()?,
        })
    }
}

impl<'de> Deserialize<'de> for ObservabilityConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = CfgrObservabilityConfig::deserialize(deserializer)?;
        let config = Self::try_from(raw).map_err(|e| D::Error::custom(format!("{:?}", e)))?;
        Ok(config)
    }
}
