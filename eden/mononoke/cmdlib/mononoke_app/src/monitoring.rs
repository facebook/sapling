/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use services::Fb303Service;
use services::FbStatus;

mod fb303;

pub use fb303::Fb303AppExtension as MonitoringAppExtension;
pub use fb303::Fb303Args as MonitoringArgs;
// Re-eport AliveService for convenience so callers do not have to get the services dependency to
// get AliveService.
pub use services::AliveService;

/// A FB303 service that reports healthy once set_ready has been called.
#[derive(Clone)]
pub struct ReadyFlagService {
    ready: Arc<AtomicBool>,
}

impl ReadyFlagService {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::Relaxed);
    }
}

impl Fb303Service for ReadyFlagService {
    fn getStatus(&self) -> FbStatus {
        if self.ready.load(Ordering::Relaxed) {
            FbStatus::Alive
        } else {
            FbStatus::Starting
        }
    }
}
