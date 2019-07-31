// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};

use fb303::fb_status;
use fb303::server::FacebookService;
use fb303_core::server::BaseService;
use fb303_core::services::base_service::{GetNameExn, GetStatusDetailsExn, GetStatusExn};

#[derive(Clone)]
pub struct FacebookServiceImpl;

impl BaseService for FacebookServiceImpl {
    fn getName(&self) -> BoxFuture<String, GetNameExn> {
        Ok("Mononoke API Server".to_string()).into_future().boxify()
    }

    fn getStatus(&self) -> BoxFuture<fb_status, GetStatusExn> {
        Ok(fb_status::ALIVE).into_future().boxify()
    }
    fn getStatusDetails(&self) -> BoxFuture<String, GetStatusDetailsExn> {
        Ok("Alive and running.".to_string()).into_future().boxify()
    }
}

impl FacebookService for FacebookServiceImpl {}
