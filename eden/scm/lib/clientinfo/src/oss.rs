/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct FbClientInfo {}

impl FbClientInfo {
    pub fn tw_job(&self) -> Option<&str> {
        None
    }

    pub fn tw_task(&self) -> Option<&str> {
        None
    }

    pub fn sandcastle_nonce(&self) -> Option<&str> {
        None
    }

    pub fn sandcastle_alias(&self) -> Option<&str> {
        None
    }
}

pub fn get_fb_client_info() -> FbClientInfo {
    FbClientInfo {}
}
