/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use fb303::fb_status;
use fb303::server::FacebookService;
use fb303_core::server::BaseService;
use fb303_core::services::base_service::{GetNameExn, GetStatusDetailsExn, GetStatusExn};

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

    async fn getStatus(&self) -> Result<fb_status, GetStatusExn> {
        if !self.will_exit.load(Ordering::Relaxed) {
            Ok(fb_status::ALIVE)
        } else {
            Ok(fb_status::STOPPING)
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

pub struct FacebookServiceImpl;

impl FacebookService for FacebookServiceImpl {}
