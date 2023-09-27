/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod request_info;

use anyhow::anyhow;
use anyhow::Result;
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

pub use crate::request_info::get_client_request_info;
pub use crate::request_info::ClientEntryPoint;
pub use crate::request_info::ClientRequestInfo;

#[derive(Default, Clone, Deserialize, Serialize, Debug)]
pub struct ClientInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(flatten)]
    pub fb: FbClientInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_info: Option<ClientRequestInfo>,
}

impl ClientInfo {
    pub fn new() -> Result<Self> {
        let fb = get_fb_client_info();

        let hostname = get_hostname().ok();
        let cri = get_client_request_info();

        Ok(ClientInfo {
            hostname,
            fb,
            request_info: Some(cri),
        })
    }

    pub fn default_with_entry_point(entry_point: ClientEntryPoint) -> Self {
        let mut client_info = Self::default();
        client_info.add_request_info(ClientRequestInfo::new(entry_point));
        client_info
    }

    pub fn into_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| anyhow!(e))
    }

    pub fn add_request_info(&mut self, info: ClientRequestInfo) -> &mut Self {
        self.request_info = Some(info);
        self
    }
}
