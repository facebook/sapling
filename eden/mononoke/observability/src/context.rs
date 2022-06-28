/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use slog::Level;
use std::sync::Arc;
use std::sync::Mutex;

use crate::config::ObservabilityConfig;
use crate::config::ScubaVerbosityLevel;
use crate::scuba::should_log_scuba_sample;
use crate::scuba::ScubaLoggingDecisionFields;

const CONFIGERATOR_OBSERVABILITY_CONFIG: &str = "scm/mononoke/observability/observability_config";

struct CfgrObservabilityContextInner {
    config_handle: ConfigHandle<ObservabilityConfig>,
}

impl CfgrObservabilityContextInner {
    fn new(config_store: &ConfigStore) -> Result<Self, Error> {
        let config_handle = config_store
            .get_config_handle_DEPRECATED(CONFIGERATOR_OBSERVABILITY_CONFIG.to_string())?;

        Ok(Self { config_handle })
    }

    fn get_logging_level(&self) -> Level {
        let config = self.config_handle.get();
        config.slog_config.level
    }

    fn should_log_scuba_sample(
        &self,
        verbosity_level: ScubaVerbosityLevel,
        logging_decision_fields: ScubaLoggingDecisionFields,
    ) -> bool {
        let config = self.config_handle.get();
        let scuba_config = &config.scuba_config;
        should_log_scuba_sample(verbosity_level, scuba_config, logging_decision_fields)
    }
}

/// A modifiable struct to be used in
/// the unit tests
pub struct TestObservabilityContextInner {
    level: Level,
}

impl TestObservabilityContextInner {
    pub fn new(level: Level) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self { level }))
    }

    pub fn set_logging_level(&mut self, level: Level) {
        self.level = level;
    }

    fn get_logging_level(&self) -> Level {
        self.level
    }

    fn should_log_scuba_sample(
        &self,
        _verbosity_level: ScubaVerbosityLevel,
        _logging_decision_fields: ScubaLoggingDecisionFields,
    ) -> bool {
        true
    }
}

/// A static `ObservabilityContext` to represent
/// traditional behavior with predefined log levels
#[derive(Debug, Clone)]
pub struct StaticObservabilityContextInner {
    level: Level,
}

impl StaticObservabilityContextInner {
    fn new(level: Level) -> Self {
        Self { level }
    }

    fn get_logging_level(&self) -> Level {
        self.level
    }

    fn should_log_scuba_sample(
        &self,
        verbosity_level: ScubaVerbosityLevel,
        _logging_decision_fields: ScubaLoggingDecisionFields,
    ) -> bool {
        verbosity_level == ScubaVerbosityLevel::Normal
    }
}

#[derive(Clone)]
enum ObservabilityContextInner {
    Dynamic(Arc<CfgrObservabilityContextInner>),
    Static(StaticObservabilityContextInner),
    Test(Arc<Mutex<TestObservabilityContextInner>>),
}

impl ObservabilityContextInner {
    fn new(config_store: &ConfigStore) -> Result<Self, Error> {
        Ok(Self::Dynamic(Arc::new(CfgrObservabilityContextInner::new(
            config_store,
        )?)))
    }

    fn new_static(level: Level) -> Self {
        Self::Static(StaticObservabilityContextInner::new(level))
    }

    fn new_test(inner: Arc<Mutex<TestObservabilityContextInner>>) -> Self {
        Self::Test(inner)
    }

    fn get_logging_level(&self) -> Level {
        match self {
            Self::Dynamic(octx) => octx.get_logging_level(),
            Self::Test(octx) => octx.lock().expect("poisoned lock").get_logging_level(),
            Self::Static(octx) => octx.get_logging_level(),
        }
    }

    pub fn should_log_scuba_sample(
        &self,
        verbosity_level: ScubaVerbosityLevel,
        logging_decision_fields: ScubaLoggingDecisionFields,
    ) -> bool {
        match self {
            Self::Dynamic(octx) => {
                octx.should_log_scuba_sample(verbosity_level, logging_decision_fields)
            }
            Self::Static(octx) => {
                octx.should_log_scuba_sample(verbosity_level, logging_decision_fields)
            }
            Self::Test(octx) => octx
                .lock()
                .expect("poiosoned lock")
                .should_log_scuba_sample(verbosity_level, logging_decision_fields),
        }
    }
}

#[derive(Clone)]
pub struct ObservabilityContext {
    inner: ObservabilityContextInner,
}

impl ObservabilityContext {
    pub fn new(config_store: &ConfigStore) -> Result<Self, Error> {
        Ok(Self {
            inner: ObservabilityContextInner::new(config_store)?,
        })
    }

    pub fn new_test(inner: Arc<Mutex<TestObservabilityContextInner>>) -> Self {
        Self {
            inner: ObservabilityContextInner::new_test(inner),
        }
    }

    pub fn new_static(level: Level) -> Self {
        Self {
            inner: ObservabilityContextInner::new_static(level),
        }
    }

    pub fn get_logging_level(&self) -> Level {
        self.inner.get_logging_level()
    }

    pub fn should_log_scuba_sample(
        &self,
        verbosity_level: ScubaVerbosityLevel,
        logging_decision_fields: ScubaLoggingDecisionFields,
    ) -> bool {
        self.inner
            .should_log_scuba_sample(verbosity_level, logging_decision_fields)
    }
}
