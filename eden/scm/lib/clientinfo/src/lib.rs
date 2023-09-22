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
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
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

#[derive(Default, Clone, Deserialize, Serialize, Debug)]
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

/// ClientRequestInfo holds information that will be used for tracing the request
/// through Source Control systems.
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct ClientRequestInfo {
    /// Identifier indicates who triggered the request (e.g: "user:user_id")
    pub main_id: Option<String>,
    /// The entry point of the request
    pub entry_point: ClientEntryPoint,
    /// A random string that identifies the request
    pub correlator: String,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub enum ClientEntryPoint {
    Sapling,
    EdenFS,
    SCS,
    SCMQuery,
    EdenAPI,
    LandService,
    LFS,
    DerivedDataService,
    ISL,
}

impl ClientRequestInfo {
    pub fn new(entry_point: ClientEntryPoint) -> Self {
        let correlator = Self::generate_correlator();
        Self::new_with_correlator(entry_point, correlator)
    }

    pub fn new_with_correlator(entry_point: ClientEntryPoint, correlator: String) -> Self {
        Self {
            main_id: None,
            entry_point,
            correlator,
        }
    }

    pub fn set_main_id(&mut self, main_id: String) {
        self.main_id = Some(main_id);
    }

    pub fn has_main_id(&self) -> bool {
        self.main_id.is_some()
    }

    fn generate_correlator() -> String {
        thread_rng()
            .sample_iter(Alphanumeric)
            .take(16)
            .map(char::from)
            .collect()
    }
}

impl Display for ClientEntryPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            ClientEntryPoint::Sapling => "sapling",
            ClientEntryPoint::EdenFS => "edenfs",
            ClientEntryPoint::SCS => "scs",
            ClientEntryPoint::SCMQuery => "scm_query",
            ClientEntryPoint::EdenAPI => "eden_api",
            ClientEntryPoint::LandService => "landservice",
            ClientEntryPoint::LFS => "lfs",
            ClientEntryPoint::DerivedDataService => "derived_data_service",
            ClientEntryPoint::ISL => "isl",
        };
        write!(f, "{}", out)
    }
}

impl TryFrom<&str> for ClientEntryPoint {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "sapling" => Ok(ClientEntryPoint::Sapling),
            "edenfs" => Ok(ClientEntryPoint::EdenFS),
            "scs" => Ok(ClientEntryPoint::SCS),
            "scm_query" => Ok(ClientEntryPoint::SCMQuery),
            "eden_api" => Ok(ClientEntryPoint::EdenAPI),
            "landservice" => Ok(ClientEntryPoint::LandService),
            "lfs" => Ok(ClientEntryPoint::LFS),
            "derived_data_service" => Ok(ClientEntryPoint::DerivedDataService),
            "isl" => Ok(ClientEntryPoint::ISL),
            _ => Err(anyhow!("Invalid client entry point")),
        }
    }
}
