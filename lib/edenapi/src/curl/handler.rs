// Copyright Facebook, Inc. 2019

use curl::easy::{Handler, WriteError};

use crate::progress::{ProgressStats, ProgressUpdater};

/// Simple Handler that just writes all received data to an internal buffer.
pub(super) struct Collector {
    data: Vec<u8>,
    updater: Option<ProgressUpdater>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            updater: None,
        }
    }

    pub fn with_progress(updater: ProgressUpdater) -> Self {
        Self {
            data: Vec::new(),
            updater: Some(updater),
        }
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.data.extend_from_slice(data);
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        if log::log_enabled!(log::Level::Trace) {
            let line = String::from_utf8_lossy(data);
            log::trace!("Received header: {:?}", line);
        }
        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        if let Some(ref updater) = self.updater {
            let dltotal = dltotal as usize;
            let dlnow = dlnow as usize;
            let ultotal = ultotal as usize;
            let ulnow = ulnow as usize;
            let stats = ProgressStats::new(dlnow, ulnow, dltotal, ultotal);
            updater.update(stats);
        }
        true
    }
}
