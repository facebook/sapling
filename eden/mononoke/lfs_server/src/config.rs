/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::{Deserialize, Serialize};
use std::default::Default;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub counter: String,
    pub limit: i64,
    pub sleep_ms: i64,
}

/// Struct representing actual config data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub track_bytes_sent: bool,
    pub enable_consistent_routing: bool,
    pub disable_hostname_logging: bool,
    pub throttle_limits: Vec<Limit>,
    pub acl_check: bool,
    pub enforce_acl_check: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            track_bytes_sent: false,
            enable_consistent_routing: false,
            disable_hostname_logging: false,
            throttle_limits: vec![],
            acl_check: false,
            enforce_acl_check: false,
        }
    }
}
