/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Error;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;

use crate::config::ObservabilityConfig;
use crate::config::ScubaVerbosityLevel;
use crate::scuba::ScubaLoggingDecisionFields;
use crate::scuba::should_log_scuba_sample;

const CONFIGERATOR_OBSERVABILITY_CONFIG: &str = "scm/mononoke/observability/observability_config";

fn load_config_handle(
    config_store: &ConfigStore,
) -> Result<ConfigHandle<ObservabilityConfig>, Error> {
    config_store.get_config_handle(CONFIGERATOR_OBSERVABILITY_CONFIG.to_string())
}

struct CfgrObservabilityContextInner {
    config_handle: Arc<ConfigHandle<ObservabilityConfig>>,
}

impl CfgrObservabilityContextInner {
    fn new(config_handle: Arc<ConfigHandle<ObservabilityConfig>>) -> Self {
        Self { config_handle }
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
pub struct TestObservabilityContextInner;

impl TestObservabilityContextInner {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self))
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
#[allow(unused)]
pub struct StaticObservabilityContextInner;

impl StaticObservabilityContextInner {
    fn new() -> Self {
        Self
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
    fn new(config_handle: Arc<ConfigHandle<ObservabilityConfig>>) -> Self {
        Self::Dynamic(Arc::new(CfgrObservabilityContextInner::new(config_handle)))
    }

    fn new_static() -> Self {
        Self::Static(StaticObservabilityContextInner::new())
    }

    fn new_test(inner: Arc<Mutex<TestObservabilityContextInner>>) -> Self {
        Self::Test(inner)
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
    config_handle: Option<Arc<ConfigHandle<ObservabilityConfig>>>,
}

impl ObservabilityContext {
    pub fn new(config_store: &ConfigStore) -> Result<Self, Error> {
        let config_handle = Arc::new(load_config_handle(config_store)?);
        Ok(Self {
            inner: ObservabilityContextInner::new(config_handle.clone()),
            config_handle: Some(config_handle),
        })
    }

    pub fn new_test(inner: Arc<Mutex<TestObservabilityContextInner>>) -> Self {
        Self {
            inner: ObservabilityContextInner::new_test(inner),
            config_handle: None,
        }
    }

    pub fn new_static(config_store: &ConfigStore) -> Self {
        Self {
            inner: ObservabilityContextInner::new_static(),
            config_handle: load_config_handle(config_store).ok().map(Arc::new),
        }
    }

    pub fn observability_config(&self) -> Option<Arc<ObservabilityConfig>> {
        self.config_handle.as_ref().map(|handle| handle.get())
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use cached_config::ConfigStore;
    use cached_config::TestSource;
    use mononoke_macros::mononoke;

    use super::*;

    // A missing config must not fail construction — else server startup breaks
    // in test/bootstrap environments that don't ship it.
    #[mononoke::test]
    fn new_static_tolerates_missing_config() {
        let source = Arc::new(TestSource::new());
        let store = ConfigStore::new(source, Duration::from_secs(60), None);

        assert!(
            ObservabilityContext::new_static(&store)
                .observability_config()
                .is_none()
        );
    }
}
