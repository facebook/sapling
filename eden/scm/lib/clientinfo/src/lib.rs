/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use hostname::get_hostname;
use serde::Deserialize;
use serde::Serialize;

pub const CLIENT_INFO_HEADER: &str = "X-Client-Info";

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;
use facebook::get_fb_client_info;
use facebook::FbClientInfo;
#[cfg(not(fbcode_build))]
use oss as facebook;

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct ClientInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u64token: Option<u64>, // Currently not used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(flatten)]
    pub fb: FbClientInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_info: Option<ClientRequestInfo>,
}

impl ClientInfo {
    pub fn new(config: &dyn Config) -> Result<Self> {
        let fb = get_fb_client_info();

        let u64token = config.get_opt::<u64>("clientinfo", "u64token")?;
        let hostname = get_hostname().ok();

        Ok(ClientInfo {
            u64token,
            hostname,
            fb,
            request_info: None,
        })
    }

    pub fn into_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| anyhow!(e))
    }

    pub fn add_request_info(&mut self, info: ClientRequestInfo) -> &mut Self {
        self.request_info = Some(info);
        self
    }
}

/// ClientRequestInfo holds information that will be used for tracing the request
/// through Source Control systems.
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct ClientRequestInfo {
    /// Identifier indicates who triggered the request (e.g: "user:user_id")
    pub main_id: String,
    /// The entry point of the request
    pub entry_point: ClientEntryPoint,
    /// A random string that identifies the request
    pub correlator: String,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub enum ClientEntryPoint {
    Sapling,
    EdenFs,
}

impl ClientRequestInfo {
    pub fn new() -> Result<Self> {
        // a dummy client request info
        Ok(ClientRequestInfo {
            main_id: "user:test".to_string(),
            entry_point: ClientEntryPoint::Sapling,
            correlator: "123456".to_string(),
        })
    }
}

impl Display for ClientEntryPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            ClientEntryPoint::Sapling => "sapling",
            ClientEntryPoint::EdenFs => "edenfs",
        };
        write!(f, "{}", out)
    }
}
