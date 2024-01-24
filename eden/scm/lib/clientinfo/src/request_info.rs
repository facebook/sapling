/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::env::var;
use std::fmt::Display;

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;

pub const ENV_SAPLING_CLIENT_ENTRY_POINT: &str = "SAPLING_CLIENT_ENTRY_POINT";
pub const ENV_SAPLING_CLIENT_CORRELATOR: &str = "SAPLING_CLIENT_CORRELATOR";

const DEFAULT_CLIENT_ENTRY_POINT_SAPLING: ClientEntryPoint = ClientEntryPoint::Sapling;
const DEFAULT_CLIENT_ENTRY_POINT_EDENFS: ClientEntryPoint = ClientEntryPoint::EdenFs;

// The global static ClientRequestInfo
lazy_static! {
    pub static ref CLIENT_REQUEST_INFO: ClientRequestInfo = new_client_request_info();
}

/// Get a copy of the global static ClientRequestInfo
pub fn get_client_request_info() -> ClientRequestInfo {
    CLIENT_REQUEST_INFO.clone()
}

/// Initilaizer of the global static ClientRequestInfo
fn new_client_request_info() -> ClientRequestInfo {
    let entry_point = var(ENV_SAPLING_CLIENT_ENTRY_POINT).ok();
    let correlator = var(ENV_SAPLING_CLIENT_CORRELATOR).ok();

    let entry_point: ClientEntryPoint = match entry_point {
        // We fallback to default entry point if the environment variable is invalid,
        // this behavior is to avoid panic or `Result` output type.
        Some(v) => {
            let entry_point = ClientEntryPoint::try_from(v.as_ref());
            match entry_point {
                Ok(entry_point) => entry_point,
                Err(_) => {
                    tracing::warn!(
                        "Failed to parse client entry point from env variable {}={}, default to {}",
                        ENV_SAPLING_CLIENT_ENTRY_POINT,
                        v,
                        ClientEntryPoint::Sapling,
                    );
                    DEFAULT_CLIENT_ENTRY_POINT_SAPLING
                }
            }
        }
        None => {
            if std::env::current_exe()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.contains("edenfs"))
                })
                .unwrap_or_default()
            {
                DEFAULT_CLIENT_ENTRY_POINT_EDENFS
            } else {
                DEFAULT_CLIENT_ENTRY_POINT_SAPLING
            }
        }
    };
    let correlator = correlator.unwrap_or_else(ClientRequestInfo::generate_correlator);

    tracing::info!(target: "clienttelemetry", client_entry_point=entry_point.to_string());
    tracing::info!(target: "clienttelemetry", client_correlator=correlator);

    ClientRequestInfo::new_ext(entry_point, correlator)
}

thread_local! {
    pub static CLIENT_REQUEST_INFO_THREAD_LOCAL: RefCell<Option<ClientRequestInfo>> = Default::default();
}

pub fn set_client_request_info_thread_local(cri: ClientRequestInfo) {
    CLIENT_REQUEST_INFO_THREAD_LOCAL.with(move |cri_old| *cri_old.borrow_mut() = Some(cri));
}

pub fn get_client_request_info_thread_local() -> Option<ClientRequestInfo> {
    CLIENT_REQUEST_INFO_THREAD_LOCAL.with(|cri| cri.borrow().clone())
}

/// ClientRequestInfo holds information that will be used for tracing the request
/// through Source Control systems.
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct ClientRequestInfo {
    /// Identifier indicates who triggered the request (e.g: "user:user_id")
    /// The `main_id` is generated on the server (Mononoke) side, client side
    /// does not use it.
    pub main_id: Option<String>,
    /// The entry point of the request
    pub entry_point: ClientEntryPoint,
    /// A random string that identifies the request
    pub correlator: String,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub enum ClientEntryPoint {
    Sapling,
    EdenFs,
    Fbclone,
    ScsServer,
    ScmQuery,
    EdenApi,
    LandService,
    LfsServer,
    DerivedDataService,
    DerivationWorker,
    InteractiveSmartlog,
    ScsClient,
    Walker,
    MegarepoTool,
    MegarepoBacksyncer,
    MegarepoForwardsyncer,
    MononokeAdmin,
    GitImport,
    RemoteGitImport,
    EdenApiReplay,
    MononokeHgSync,
    CurlTest,
    MirrorHgCommits,
    StreamingClone,
    ScmDaemon,
    BookmarkService,
    BookmarkServiceClientCli,
}

impl ClientRequestInfo {
    /// Create a new ClientRequestInfo with entry_point. The correlator will be a
    /// randomly generated string.
    ///
    /// NOTE: Please consider using `get_client_request_info()` if you just
    /// want to get the current singleton ClientRequestInfo object.
    pub fn new(entry_point: ClientEntryPoint) -> Self {
        let correlator = Self::generate_correlator();

        Self::new_ext(entry_point, correlator)
    }

    /// Create a new ClientRequestInfo with entry_point and correlator.
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

    pub(crate) fn generate_correlator() -> String {
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
            ClientEntryPoint::EdenFs => "edenfs",
            ClientEntryPoint::Fbclone => "fbclone",
            ClientEntryPoint::ScsServer => "scs",
            ClientEntryPoint::ScmQuery => "scm_query",
            ClientEntryPoint::EdenApi => "eden_api",
            ClientEntryPoint::LandService => "landservice",
            ClientEntryPoint::LfsServer => "lfs",
            ClientEntryPoint::DerivedDataService => "derived_data_service",
            ClientEntryPoint::DerivationWorker => "derivation_worker",
            ClientEntryPoint::InteractiveSmartlog => "isl",
            ClientEntryPoint::ScsClient => "scsc",
            ClientEntryPoint::Walker => "walker",
            ClientEntryPoint::MegarepoTool => "megarepo_tool",
            ClientEntryPoint::MegarepoBacksyncer => "megarepo_backsyncer",
            ClientEntryPoint::MegarepoForwardsyncer => "megarepo_forwardsyncer",
            ClientEntryPoint::MononokeAdmin => "mononoke_admin",
            ClientEntryPoint::GitImport => "git_import",
            ClientEntryPoint::RemoteGitImport => "remote_git_import",
            ClientEntryPoint::EdenApiReplay => "eden_api_replay",
            ClientEntryPoint::MononokeHgSync => "hg_sync",
            ClientEntryPoint::CurlTest => "curl_test",
            ClientEntryPoint::MirrorHgCommits => "mirror_hg_commits",
            ClientEntryPoint::StreamingClone => "streaming_clone",
            ClientEntryPoint::ScmDaemon => "scm_daemon",
            ClientEntryPoint::BookmarkService => "bookmark_service",
            ClientEntryPoint::BookmarkServiceClientCli => "bookmark_service_client_cli",
        };
        write!(f, "{}", out)
    }
}

impl TryFrom<&str> for ClientEntryPoint {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "sapling" => Ok(ClientEntryPoint::Sapling),
            "edenfs" => Ok(ClientEntryPoint::EdenFs),
            "fbclone" => Ok(ClientEntryPoint::Fbclone),
            "scs" => Ok(ClientEntryPoint::ScsServer),
            "scm_query" => Ok(ClientEntryPoint::ScmQuery),
            "eden_api" => Ok(ClientEntryPoint::EdenApi),
            "landservice" => Ok(ClientEntryPoint::LandService),
            "lfs" => Ok(ClientEntryPoint::LfsServer),
            "derived_data_service" => Ok(ClientEntryPoint::DerivedDataService),
            "derivation_worker" => Ok(ClientEntryPoint::DerivationWorker),
            "isl" => Ok(ClientEntryPoint::InteractiveSmartlog),
            "scsc" => Ok(ClientEntryPoint::ScsClient),
            "walker" => Ok(ClientEntryPoint::Walker),
            "megarepo_tool" => Ok(ClientEntryPoint::MegarepoTool),
            "megarepo_backsyncer" => Ok(ClientEntryPoint::MegarepoBacksyncer),
            "megarepo_forwardsyncer" => Ok(ClientEntryPoint::MegarepoForwardsyncer),
            "mononoke_admin" => Ok(ClientEntryPoint::MononokeAdmin),
            "git_import" => Ok(ClientEntryPoint::GitImport),
            "remote_git_import" => Ok(ClientEntryPoint::RemoteGitImport),
            "eden_api_replay" => Ok(ClientEntryPoint::EdenApiReplay),
            "hg_sync" => Ok(ClientEntryPoint::MononokeHgSync),
            "curl_test" => Ok(ClientEntryPoint::CurlTest),
            "mirror_hg_commits" => Ok(ClientEntryPoint::MirrorHgCommits),
            "streaming_clone" => Ok(ClientEntryPoint::StreamingClone),
            "scm_daemon" => Ok(ClientEntryPoint::ScmDaemon),
            "bookmark_service" => Ok(ClientEntryPoint::BookmarkService),
            "bookmark_service_client_clie" => Ok(ClientEntryPoint::BookmarkServiceClientCli),
            _ => Err(anyhow!("Invalid client entry point")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env::set_var;

    use super::*;

    #[test]
    fn test_client_request_info() {
        let mut cri = ClientRequestInfo::new(ClientEntryPoint::Sapling);
        assert_eq!(cri.main_id, None);
        assert_eq!(cri.entry_point, ClientEntryPoint::Sapling);
        assert!(!cri.correlator.is_empty());
        assert!(!cri.has_main_id());

        let correlator = "test1234".to_owned();
        let main_id = "user:test".to_owned();
        let entry_point = ClientEntryPoint::EdenApi;
        cri.set_main_id(main_id.clone());
        cri.set_entry_point(entry_point);
        cri.set_correlator(correlator.clone());

        assert_eq!(cri.main_id, Some(main_id));
        assert_eq!(cri.entry_point, ClientEntryPoint::EdenApi);
        assert_eq!(cri.correlator, correlator);
        assert!(cri.has_main_id());
    }

    #[test]
    fn test_static_client_requst_info_with_env_vars() {
        let correlator = "test1234";
        set_var(ENV_SAPLING_CLIENT_CORRELATOR, correlator);
        set_var(ENV_SAPLING_CLIENT_ENTRY_POINT, "isl");
        let cri = get_client_request_info();
        assert_eq!(cri.entry_point, ClientEntryPoint::InteractiveSmartlog);
        assert_eq!(cri.correlator, correlator.to_owned());
    }

    #[test]
    fn test_client_entry_point() {
        assert_eq!(
            Some(ClientEntryPoint::Sapling),
            ClientEntryPoint::try_from(ClientEntryPoint::Sapling.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::EdenFs),
            ClientEntryPoint::try_from(ClientEntryPoint::EdenFs.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::Fbclone),
            ClientEntryPoint::try_from(ClientEntryPoint::Fbclone.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::ScsServer),
            ClientEntryPoint::try_from(ClientEntryPoint::ScsServer.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::ScmQuery),
            ClientEntryPoint::try_from(ClientEntryPoint::ScmQuery.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::EdenApi),
            ClientEntryPoint::try_from(ClientEntryPoint::EdenApi.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::LandService),
            ClientEntryPoint::try_from(ClientEntryPoint::LandService.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::LfsServer),
            ClientEntryPoint::try_from(ClientEntryPoint::LfsServer.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::DerivedDataService),
            ClientEntryPoint::try_from(ClientEntryPoint::DerivedDataService.to_string().as_ref())
                .ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::DerivationWorker),
            ClientEntryPoint::try_from(ClientEntryPoint::DerivationWorker.to_string().as_ref())
                .ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::InteractiveSmartlog),
            ClientEntryPoint::try_from(ClientEntryPoint::InteractiveSmartlog.to_string().as_ref())
                .ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::ScsClient),
            ClientEntryPoint::try_from(ClientEntryPoint::ScsClient.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::Walker),
            ClientEntryPoint::try_from(ClientEntryPoint::Walker.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MegarepoTool),
            ClientEntryPoint::try_from(ClientEntryPoint::MegarepoTool.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MegarepoBacksyncer),
            ClientEntryPoint::try_from(ClientEntryPoint::MegarepoBacksyncer.to_string().as_ref())
                .ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MegarepoForwardsyncer),
            ClientEntryPoint::try_from(
                ClientEntryPoint::MegarepoForwardsyncer.to_string().as_ref()
            )
            .ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MononokeAdmin),
            ClientEntryPoint::try_from(ClientEntryPoint::MononokeAdmin.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::GitImport),
            ClientEntryPoint::try_from(ClientEntryPoint::GitImport.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::RemoteGitImport),
            ClientEntryPoint::try_from(ClientEntryPoint::RemoteGitImport.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::EdenApiReplay),
            ClientEntryPoint::try_from(ClientEntryPoint::EdenApiReplay.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MononokeHgSync),
            ClientEntryPoint::try_from(ClientEntryPoint::MononokeHgSync.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::CurlTest),
            ClientEntryPoint::try_from(ClientEntryPoint::CurlTest.to_string().as_ref()).ok()
        );
        assert_eq!(
            Some(ClientEntryPoint::MirrorHgCommits),
            ClientEntryPoint::try_from(ClientEntryPoint::MirrorHgCommits.to_string().as_ref()).ok()
        );

        assert_eq!(
            Some(ClientEntryPoint::StreamingClone),
            ClientEntryPoint::try_from(ClientEntryPoint::StreamingClone.to_string().as_ref()).ok()
        );

        assert_eq!(
            Some(ClientEntryPoint::ScmDaemon),
            ClientEntryPoint::try_from(ClientEntryPoint::ScmDaemon.to_string().as_ref()).ok()
        );
    }
}
