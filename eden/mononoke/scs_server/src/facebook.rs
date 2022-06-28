/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use fb303_core::fb303_status;
use fb303_core::server::BaseService;
use fb303_core::services::base_service::GetNameExn;
use fb303_core::services::base_service::GetStatusDetailsExn;
use fb303_core::services::base_service::GetStatusExn;

#[derive(Clone)]
pub struct BaseServiceImpl {
    will_exit: Arc<AtomicBool>,
}

impl BaseServiceImpl {
    pub fn new(will_exit: Arc<AtomicBool>) -> Self {
        Self { will_exit }
    }
}

#[async_trait]
impl BaseService for BaseServiceImpl {
    async fn getName(&self) -> Result<String, GetNameExn> {
        Ok("Mononoke Source Control Service Server".to_string())
    }

    async fn getStatus(&self) -> Result<fb303_status, GetStatusExn> {
        if !self.will_exit.load(Ordering::Relaxed) {
            Ok(fb303_status::ALIVE)
        } else {
            Ok(fb303_status::STOPPING)
        }
    }

    async fn getStatusDetails(&self) -> Result<String, GetStatusDetailsExn> {
        if !self.will_exit.load(Ordering::Relaxed) {
            Ok("Alive and running.".to_string())
        } else {
            Ok("Shutting down.".to_string())
        }
    }
}
