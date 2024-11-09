/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

pub fn log_cross_environment_session_id() -> String {
    String::new()
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
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

    pub fn sandcastle_type(&self) -> Option<&str> {
        None
    }
}

pub fn get_fb_client_info() -> FbClientInfo {
    FbClientInfo {}
}
