// Copyright Facebook, Inc. 2019

use curl::easy::{Handler, WriteError};

use crate::progress::{ProgressHandle, ProgressStats};

/// Simple Handler that just writes all received data to an internal buffer.
pub(super) struct Collector {
    data: Vec<u8>,
    progress: Option<ProgressHandle>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            progress: None,
        }
    }

    pub fn with_progress(progress: ProgressHandle) -> Self {
        Self {
            data: Vec::new(),
            progress: Some(progress),
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
        if let Some(ref progress) = self.progress {
            let dltotal = dltotal as usize;
            let dlnow = dlnow as usize;
            let ultotal = ultotal as usize;
            let ulnow = ulnow as usize;
            let stats = ProgressStats::new(dlnow, ulnow, dltotal, ultotal);
            progress.update(stats);
        }
        true
    }
}
