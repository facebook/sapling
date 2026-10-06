/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `EdenFsInstance` pairs the daemon-independent instance from `edenfs-core`
//! with the thriftclient-backed [`EdenFsClient`]. It is designed to be
//! initialized once and accessed globally throughout your application.
//!
//! It derefs to [`edenfs_core::instance::EdenFsInstance`], so all the
//! config/checkout accessors documented there are available on this type,
//! and `&EdenFsInstance` coerces wherever a `&edenfs_core::instance::EdenFsInstance`
//! is expected.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use edenfs_core::daemon::EdenFsDaemon;
pub use edenfs_core::instance::DEFAULT_CONFIG_DIR;
pub use edenfs_core::instance::DEFAULT_ETC_EDEN_DIR;
use fbinit::expect_init;

use crate::client::EdenFsClient;
use crate::use_case::UseCase;
use crate::use_case::UseCaseId;

#[derive(Clone)]
pub struct EdenFsInstance {
    core: edenfs_core::instance::EdenFsInstance,
    client: Arc<EdenFsClient>,
}

impl fmt::Debug for EdenFsInstance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.core.fmt(f)
    }
}

impl EdenFsInstance {
    /// Creates a new `EdenFsInstance` with the specified paths.
    ///
    /// # Parameters
    ///
    /// * `use_case_id` - A unique identifier for a use case - used to access configuration settings and attribute usage to a given use case.
    /// * `config_dir` - Path to the EdenFS configuration directory
    /// * `etc_eden_dir` - Path to the system-wide EdenFS configuration directory
    /// * `home_dir` - Optional path to the user's home directory
    pub fn new(
        use_case_id: UseCaseId,
        config_dir: PathBuf,
        etc_eden_dir: PathBuf,
        home_dir: Option<PathBuf>,
    ) -> EdenFsInstance {
        let socketfile = config_dir.join("socket");
        let use_case = Arc::new(UseCase::new(&config_dir, use_case_id));
        let client = Arc::new(EdenFsClient::new(expect_init(), use_case, socketfile));
        let daemon: Arc<dyn EdenFsDaemon> = client.clone();
        Self {
            core: edenfs_core::instance::EdenFsInstance::with_daemon(
                config_dir,
                etc_eden_dir,
                home_dir,
                daemon,
            ),
            client,
        }
    }

    /// Returns an `Arc<EdenFsClient>` for interacting with EdenFS.
    ///
    /// This method returns a ref counted client that connects to the EdenFS
    /// daemon using the socket file path from this instance.
    pub fn get_client(&self) -> Arc<EdenFsClient> {
        self.client.clone()
    }
}

impl std::ops::Deref for EdenFsInstance {
    type Target = edenfs_core::instance::EdenFsInstance;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
