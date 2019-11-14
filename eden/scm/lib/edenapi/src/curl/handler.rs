/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blackbox::{
    event::{Event, NetworkOp},
    log,
};
use curl::easy::{Handler, WriteError};
use lazy_static::lazy_static;
use regex::Regex;
use url::Url;

use crate::progress::{ProgressStats, ProgressUpdater};

lazy_static! {
    static ref STATUS_RE: Regex = Regex::new(r"(?i)HTTP/[0-9.]+ ([0-9]+)").unwrap();
    static ref SESSION_ID_RE: Regex = Regex::new(r"(?i)x-session-id: ([a-z0-9-]+)").unwrap();
}

#[derive(Default)]
pub struct DraftEvent {
    url: Option<String>,
    session_id: Option<String>,
    status: u32,
    downloaded: f64,
    uploaded: f64,
}

impl From<&Url> for DraftEvent {
    fn from(url: &Url) -> Self {
        Self {
            url: Some(url.to_string()),
            ..Default::default()
        }
    }
}

/// Simple Handler that just writes all received data to an internal buffer.
pub(super) struct Collector {
    data: Vec<u8>,
    updater: Option<ProgressUpdater>,
    event: DraftEvent,
}

impl Collector {
    pub fn new(event: impl Into<DraftEvent>) -> Self {
        Self {
            data: Vec::new(),
            updater: None,
            event: event.into(),
        }
    }

    pub fn with_progress(event: impl Into<DraftEvent>, updater: ProgressUpdater) -> Self {
        let mut ret = Self::new(event);
        ret.updater = Some(updater);
        ret
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
        let line = String::from_utf8_lossy(data);

        if log::log_enabled!(log::Level::Trace) {
            log::trace!("Received header: {:?}", line);
        }

        // NOTE: unwrapping is safe below: if our regex matches, then it'll capture.

        if let Some(capture) = STATUS_RE.captures(&line) {
            if let Ok(status) = capture.get(1).unwrap().as_str().parse() {
                // If we can parse the status code, then let's use it (if we can't ... that's too
                // bad but there isn't much we can do about it).
                self.event.status = status;
            }
        }

        if let Some(capture) = SESSION_ID_RE.captures(&line) {
            self.event.session_id = Some(capture.get(1).unwrap().as_str().to_owned());
        }

        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        self.event.downloaded = dltotal;
        self.event.uploaded = ultotal;

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

impl Drop for Collector {
    fn drop(&mut self) {
        let url = self.event.url.take().unwrap_or(String::default());
        let session_id = self.event.session_id.take().unwrap_or(String::default());

        let event = Event::Network {
            op: NetworkOp::EdenApiRequest,
            read_bytes: self.event.downloaded as u64,
            write_bytes: self.event.uploaded as u64,
            calls: u64::default(),
            duration_ms: u64::default(),
            latency_ms: u64::default(),
            result: Some(self.event.status.into()),
            url,
            session_id,
        };
        log(&event);
    }
}
