/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The telemetry API that open-source builds of the EdenFS Rust crates
//! compile against. Internal builds use eden/fs/rust/facebook/edenfs-telemetry
//! instead; this crate mirrors the parts of its interface that the open-source
//! builds of those crates call, and every sample it receives is dropped.

use std::sync::Arc;

#[derive(Debug, Default, serde::Serialize)]
pub struct EdenSample {}

impl EdenSample {
    pub fn new() -> Self {
        Self {}
    }

    pub fn add_int(&mut self, _name: impl Into<String>, _value: i64) {}

    pub fn add_string(&mut self, _name: impl Into<String>, _value: impl Into<String>) {}

    pub fn add_string_list(&mut self, _name: impl Into<String>, _value: Vec<String>) {}

    pub fn add_bool(&mut self, _name: impl Into<String>, _value: bool) {}

    pub fn fail(&mut self, _error_msg: &str) {}
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum CommonData {
    Hostname,
    Platform,
    OsVersion,
    PackageVersion,
}

impl CommonData {
    pub fn default_key(&self) -> &'static str {
        match self {
            CommonData::Hostname => "hostname",
            CommonData::Platform => "platform",
            CommonData::OsVersion => "os_version",
            CommonData::PackageVersion => "package_version",
        }
    }
}

pub fn collect_system_info<F>(_sample: &mut EdenSample, _mapper: F)
where
    F: Fn(CommonData) -> &'static str,
{
}

pub fn edenfs_events_mapper(common_data: CommonData) -> &'static str {
    common_data.default_key()
}

pub trait SampleLogger: Send + Sync {
    fn log(&self, sample: EdenSample) -> anyhow::Result<()>;
}

pub struct NullLogger {}

impl SampleLogger for NullLogger {
    fn log(&self, _sample: EdenSample) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct QueueingScubaLogger {}

impl QueueingScubaLogger {
    pub fn new(_logger: Arc<dyn SampleLogger>, _queue_size: usize) -> Self {
        Self {}
    }
}

impl SampleLogger for QueueingScubaLogger {
    fn log(&self, _sample: EdenSample) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn create_logger(_data_set: String) -> Arc<dyn SampleLogger> {
    Arc::new(NullLogger {})
}
