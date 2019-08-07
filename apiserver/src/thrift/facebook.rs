// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use async_trait::async_trait;
use fb303::fb_status;
use fb303::server::FacebookService;
use fb303_core::server::BaseService;
use fb303_core::services::base_service::{GetNameExn, GetStatusDetailsExn, GetStatusExn};

#[derive(Clone)]
pub struct FacebookServiceImpl;

#[async_trait]
impl BaseService for FacebookServiceImpl {
    async fn getName(&self) -> Result<String, GetNameExn> {
        Ok("Mononoke API Server".to_string())
    }

    async fn getStatus(&self) -> Result<fb_status, GetStatusExn> {
        Ok(fb_status::ALIVE)
    }

    async fn getStatusDetails(&self) -> Result<String, GetStatusDetailsExn> {
        Ok("Alive and running.".to_string())
    }
}

impl FacebookService for FacebookServiceImpl {}
