/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use parking_lot::RwLock;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;

lazy_static! {
    pub static ref CLIENT_REQUEST_INFO: RwLock<ClientRequestInfo> = {
        let entry_point = ClientEntryPoint::Sapling;
        let correlator = thread_rng()
            .sample_iter(Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        RwLock::new(ClientRequestInfo::new_ext(entry_point, correlator))
    };
}

pub fn get_client_request_info() -> ClientRequestInfo {
    let cri = CLIENT_REQUEST_INFO.read();
    cri.clone()
}

pub fn update_client_request_info(entry_point: ClientEntryPoint, correlator: String) {
    let mut client_info = CLIENT_REQUEST_INFO.write();
    client_info.set_entry_point(entry_point);
    client_info.set_correlator(correlator);
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

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
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

        Self::new_ext(entry_point, correlator)
    }

    pub fn new_ext(entry_point: ClientEntryPoint, correlator: String) -> Self {
        Self {
            main_id: None,
            entry_point,
            correlator,
        }
    }

    pub fn set_entry_point(&mut self, entry_point: ClientEntryPoint) {
        self.entry_point = entry_point;
    }

    pub fn set_correlator(&mut self, correlator: String) {
        self.correlator = correlator;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_requst_info() {
        let mut cri = ClientRequestInfo::new(ClientEntryPoint::Sapling);
        assert_eq!(cri.main_id, None);
        assert_eq!(cri.entry_point, ClientEntryPoint::Sapling);
        assert!(!cri.correlator.is_empty());
        assert!(!cri.has_main_id());

        let correlator = "test1234".to_owned();
        let main_id = "user:test".to_owned();
        let entry_point = ClientEntryPoint::EdenAPI;
        cri.set_main_id(main_id.clone());
        cri.set_entry_point(entry_point);
        cri.set_correlator(correlator.clone());

        assert_eq!(cri.main_id, Some(main_id));
        assert_eq!(cri.entry_point, ClientEntryPoint::EdenAPI);
        assert_eq!(cri.correlator, correlator);
        assert!(cri.has_main_id());
    }

    #[test]
    fn test_static_client_requst_info() {
        let cri = get_client_request_info();
        assert!(!cri.correlator.is_empty());
        assert_eq!(cri.entry_point, ClientEntryPoint::Sapling);

        let correlator = "test1234".to_owned();
        let entry_point = ClientEntryPoint::EdenAPI;
        update_client_request_info(entry_point.clone(), correlator.clone());

        let cri = get_client_request_info();
        assert_eq!(cri.entry_point, entry_point);
        assert_eq!(cri.correlator, correlator);
    }
}
