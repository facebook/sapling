/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod request_info;

use anyhow::Context;
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
pub use crate::request_info::get_client_request_info_thread_local;
pub use crate::request_info::set_client_request_info_thread_local;
pub use crate::request_info::ClientEntryPoint;
pub use crate::request_info::ClientRequestInfo;
pub use crate::request_info::ENV_SAPLING_CLIENT_CORRELATOR;
pub use crate::request_info::ENV_SAPLING_CLIENT_ENTRY_POINT;

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(flatten)]
    pub fb: FbClientInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_info: Option<ClientRequestInfo>,
}

impl ClientInfo {
    /// Creates a new ClientInfo object with a singleton (Sapling) ClientRequestInfo
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

    /// Creates a new ClientInfo object with fresh generated ClientRequestInfo for the specified ClientEntryPoint
    pub fn new_with_entry_point(entry_point: ClientEntryPoint) -> Result<Self> {
        let fb = get_fb_client_info();
        let hostname = get_hostname().ok();
        Ok(ClientInfo {
            hostname,
            fb,
            request_info: Some(ClientRequestInfo::new(entry_point)),
        })
    }

    /// Creates a new ClientInfo object with given ClientRequestInfo
    pub fn new_with_client_request_info(client_request_info: ClientRequestInfo) -> Result<Self> {
        let fb = get_fb_client_info();
        let hostname = get_hostname().ok();
        Ok(ClientInfo {
            hostname,
            fb,
            request_info: Some(client_request_info),
        })
    }

    /// Creates a new ClientInfo object with fresh generated ClientRequestInfo for the specified
    /// ClientEntryPoint but the remaining fields will be empty.
    pub fn default_with_entry_point(entry_point: ClientEntryPoint) -> Self {
        let mut client_info = Self::default();
        client_info.add_request_info(ClientRequestInfo::new(entry_point));
        client_info
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Failed to serialize ClientInfo")
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to parse ClientInfo")
    }

    pub fn add_request_info(&mut self, info: ClientRequestInfo) -> &mut Self {
        self.request_info = Some(info);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_from_json() {
        // test checks that we can parse ClientInfo object from a json where only entry_point and
        // correlator set.
        assert!(ClientInfo::from_json(r#"{"request_info":{"entry_point":"EdenApiReplay","correlator":"vmazpnjezhjsjkay"}}"#).is_ok());
    }
}
