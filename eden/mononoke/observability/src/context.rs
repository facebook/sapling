/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cached_config::{ConfigHandle, ConfigStore};
use observability_config::types::{ObservabilityConfig, SlogLoggingLevel as CfgrLoggingLevel};
use slog::Level;
use std::sync::{Arc, Mutex};

const CONFIGERATOR_OBSERVABILITY_CONFIG: &str = "scm/mononoke/observability/observability_config";

fn cfgr_to_slog_level(level: CfgrLoggingLevel) -> Level {
    match level {
        CfgrLoggingLevel::Trace => Level::Trace,
        CfgrLoggingLevel::Debug => Level::Debug,
        CfgrLoggingLevel::Info => Level::Info,
        CfgrLoggingLevel::Warning => Level::Warning,
        CfgrLoggingLevel::Error => Level::Error,
        CfgrLoggingLevel::Critical => Level::Critical,
        other => panic!("unexpected SlogLoggingLevel: {:?}", other),
    }
}

struct CfgrObservabilityContextInner {
    config_handle: ConfigHandle<ObservabilityConfig>,
}

impl CfgrObservabilityContextInner {
    fn new(config_store: &ConfigStore) -> Result<Self, Error> {
        let config_handle =
            config_store.get_config_handle(CONFIGERATOR_OBSERVABILITY_CONFIG.to_string())?;

        Ok(Self { config_handle })
    }

    fn get_logging_level(&self) -> Level {
        let config = self.config_handle.get();
        let cfgr_level = config.slog_config.level;
        cfgr_to_slog_level(cfgr_level)
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
}
