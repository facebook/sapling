/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::RwLock;

use anyhow::Error;
use anyhow::anyhow;
use fbthrift::Deserialize;
use fbthrift::ProtocolReader;
use observability_config::ConsistentHashingType as CfgrConsistentHashingType;
use observability_config::ExperimentJustKnob as CfgrExperimentJustKnob;
use observability_config::ObservabilityConfig as CfgrObservabilityConfig;
use observability_config::ScubaObservabilityConfig as CfgrScubaObservabilityConfig;
use observability_config::ScubaVerbosityLevel as CfgrScubaVerbosityLevel;
use regex::Regex;

fn cfgr_to_scuba_level(level: &CfgrScubaVerbosityLevel) -> Result<ScubaVerbosityLevel, Error> {
    match *level {
        CfgrScubaVerbosityLevel::Normal => Ok(ScubaVerbosityLevel::Normal),
        CfgrScubaVerbosityLevel::Verbose => Ok(ScubaVerbosityLevel::Verbose),
        other => Err(anyhow!("unexpected ScubaLoggingLevel: {other:?}")),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsistentHashingType {
    NoHashing,
    Correlator,
    MainId,
}

#[derive(Clone, Debug)]
pub struct ExperimentJustKnob {
    pub jk_name: String,
    pub switch_values: Vec<String>,
    pub consistent_hashing: ConsistentHashingType,
}

pub struct ObservabilityConfig {
    pub scuba_config: ScubaObservabilityConfig,
    pub enabled_experiments_jk: Vec<ExperimentJustKnob>,
}

fn cfgr_to_consistent_hashing(
    value: &CfgrConsistentHashingType,
) -> Result<ConsistentHashingType, Error> {
    match *value {
        CfgrConsistentHashingType::NoHashing => Ok(ConsistentHashingType::NoHashing),
        CfgrConsistentHashingType::ClientCorrelator => Ok(ConsistentHashingType::Correlator),
        CfgrConsistentHashingType::ClientMainId => Ok(ConsistentHashingType::MainId),
        other => Err(anyhow!("unexpected ConsistentHashingType: {other:?}")),
    }
}

impl TryFrom<CfgrExperimentJustKnob> for ExperimentJustKnob {
    type Error = Error;

    fn try_from(value: CfgrExperimentJustKnob) -> Result<Self, Error> {
        let CfgrExperimentJustKnob {
            jk_name,
            switch_values,
            consistent_hashing,
            ..
        } = value;
        Ok(Self {
            jk_name,
            switch_values,
            consistent_hashing: cfgr_to_consistent_hashing(&consistent_hashing)?,
        })
    }
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
        let CfgrObservabilityConfig {
            scuba_config,
            enabled_experiments_jk,
            ..
        } = value;

        let enabled_experiments_jk = enabled_experiments_jk
            .into_iter()
            .map(ExperimentJustKnob::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            scuba_config: scuba_config.try_into()?,
            enabled_experiments_jk,
        })
    }
}

impl<P> Deserialize<P> for ObservabilityConfig
where
    P: ProtocolReader,
{
    fn rs_thrift_read(p: &mut P) -> Result<Self, Error> {
        let raw = CfgrObservabilityConfig::rs_thrift_read(p)?;
        Self::try_from(raw)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use cached_config::ConfigStore;
    use cached_config::ModificationTime;
    use cached_config::TestSource;
    use mononoke_macros::mononoke;

    use super::*;

    // The shape Configerator materializes: enums as ints (NoHashing=0,
    // Correlator=1, MainId=2), thrift snake_case field names.
    const SAMPLE_CONFIG: &str = r#"{
      "scuba_config": {
        "level": 1,
        "verbose_sessions": [],
        "verbose_unixnames": [],
        "verbose_source_hostnames": ["foo.*", "bar[0-9]+"]
      },
      "enabled_experiments_jk": [
        {"jk_name": "scm/mononoke:a", "switch_values": ["1", "2"], "consistent_hashing": 1},
        {"jk_name": "scm/mononoke:b", "switch_values": [], "consistent_hashing": 2},
        {"jk_name": "scm/mononoke:c", "switch_values": ["x"], "consistent_hashing": 0}
      ]
    }"#;

    #[mononoke::test]
    fn decodes_configerator_thrift_simplejson() {
        let path = "scm/mononoke/observability/observability_config";
        let source = Arc::new(TestSource::new());
        source.insert_config(path, SAMPLE_CONFIG, ModificationTime::UnixTimestamp(1));
        let store = ConfigStore::new(source, Duration::from_secs(60), None);

        let handle = store
            .get_config_handle::<ObservabilityConfig>(path.to_string())
            .expect("observability config must decode via fbthrift simple_json");
        let config = handle.get();

        assert_eq!(config.scuba_config.level, ScubaVerbosityLevel::Verbose);
        assert_eq!(
            config
                .scuba_config
                .verbose_source_hostname_regexes
                .read()
                .unwrap()
                .len(),
            2
        );

        let jks = &config.enabled_experiments_jk;
        assert_eq!(jks.len(), 3);
        assert_eq!(jks[0].jk_name, "scm/mononoke:a");
        assert_eq!(jks[0].switch_values, vec!["1".to_string(), "2".to_string()]);
        assert_eq!(jks[0].consistent_hashing, ConsistentHashingType::Correlator);
        assert!(jks[1].switch_values.is_empty());
        assert_eq!(jks[1].consistent_hashing, ConsistentHashingType::MainId);
        assert_eq!(jks[2].consistent_hashing, ConsistentHashingType::NoHashing);
    }
}
