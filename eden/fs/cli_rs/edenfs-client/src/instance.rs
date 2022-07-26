/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFsInstance - manages EdenFS resources besides Thrift connection (managed by
//! [`EdenFsClient`]).

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;

use edenfs_config::EdenFsConfig;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use fbthrift_socket::SocketTransport;
use thrift_types::edenfs::client::EdenService;
use thrift_types::edenfs::types::DaemonInfo;
use thrift_types::fb303_core::types::fb303_status;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;
use tracing::event;
use tracing::Level;

use crate::utils::get_executable;
use crate::EdenFsClient;

/// These paths are relative to the user's client directory.
const CLIENTS_DIR: &str = "clients";
const CONFIG_JSON: &str = "config.json";

#[derive(Debug)]
pub struct EdenFsInstance {
    config_dir: PathBuf,
    etc_eden_dir: PathBuf,
    home_dir: Option<PathBuf>,
}

impl EdenFsInstance {
    pub fn new(config_dir: PathBuf, etc_eden_dir: PathBuf, home_dir: Option<PathBuf>) -> Self {
        event!(
            Level::TRACE,
            ?config_dir,
            ?etc_eden_dir,
            ?home_dir,
            "Creating EdenFsInstance"
        );
        Self {
            config_dir,
            etc_eden_dir,
            home_dir,
        }
    }

    pub fn get_config(&self) -> Result<EdenFsConfig> {
        edenfs_config::load_config(
            &self.etc_eden_dir,
            self.home_dir.as_ref().map(|x| x.as_ref()),
        )
    }

    pub fn get_user_home_dir(&self) -> Option<&PathBuf> {
        self.home_dir.as_ref()
    }

    pub fn should_prefetch_profiles(&self) -> bool {
        self.get_config()
            .ok()
            .map_or(false, |config| config.prefetch_profiles.prefetching_enabled)
    }

    pub fn should_prefetch_predictive_profiles(&self) -> bool {
        self.get_config().ok().map_or(false, |config| {
            config.prefetch_profiles.predictive_prefetching_enabled
        })
    }

    async fn _connect(&self, socket_path: &PathBuf) -> Result<EdenFsClient> {
        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(EdenFsError::ThriftIoError)?;
        let transport = SocketTransport::new(stream);
        let client = <dyn EdenService>::new(BinaryProtocol, transport);

        Ok(client)
    }

    pub async fn connect(&self, timeout: Option<Duration>) -> Result<EdenFsClient> {
        let socket_path = self.config_dir.join("socket");

        let connect = self._connect(&socket_path);
        let res = if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, connect)
                .await
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(socket_path))?
        } else {
            connect.await
        };

        res
    }

    #[cfg(windows)]
    fn pidfile(&self) -> PathBuf {
        self.config_dir.join("pid")
    }

    #[cfg(unix)]
    fn pidfile(&self) -> PathBuf {
        self.config_dir.join("lock")
    }

    /// Read the pid from the EdenFS lockfile
    fn pid(&self) -> Result<sysinfo::Pid, anyhow::Error> {
        let pidfile = self.pidfile();
        let pid_bytes = std::fs::read(&pidfile)
            .with_context(|| format!("Unable to read from pid file '{}'", pidfile.display()))?;
        let pid_str =
            std::str::from_utf8(&pid_bytes).context("Unable to parse pid file as UTF-8 string")?;

        pid_str
            .trim()
            .parse()
            .with_context(|| format!("Unable to parse pid file content: '{}'", pid_str))
    }

    /// Retrieving running EdenFS process status based on lock file
    pub fn status_from_lock(&self) -> Result<i32, anyhow::Error> {
        let pid = self.pid()?;

        let exe = match get_executable(pid) {
            Some(exe) => exe,
            None => {
                tracing::debug!("PID {} is not running", pid);
                return Err(anyhow!("EdenFS is not running"));
            }
        };
        let name = match exe.file_name() {
            Some(name) => name.to_string_lossy(),
            None => {
                tracing::debug!("Unable to retrieve information about PID {}", pid);
                return Err(anyhow!("EdenFS is not running"));
            }
        };

        tracing::trace!(?name, "executable name");

        if name == "edenfs"
            || name == "fake_edenfs"
            || (cfg!(windows) && name.ends_with("edenfs.exe"))
        {
            Err(anyhow!(
                "EdenFS's Thrift server does not appear to be running, \
                but the process is still alive (PID={})",
                pid
            ))
        } else {
            Err(anyhow!("EdenFS is not running"))
        }
    }

    pub async fn get_health(&self, timeout: Option<Duration>) -> Result<DaemonInfo> {
        let client = self
            .connect(timeout.or_else(|| Some(Duration::from_secs(3))))
            .await
            .context("Unable to connect to EdenFS daemon")?;
        event!(Level::DEBUG, "connected to EdenFS daemon");
        client.getDaemonInfo().await.from_err()
    }

    /// Returns a map of mount paths to mount names
    /// as defined in EdenFS's config.json.
    pub fn get_configured_mounts_map(&self) -> Result<BTreeMap<PathBuf, String>, anyhow::Error> {
        let directory_map = self.config_dir.join(CONFIG_JSON);
        match std::fs::read_to_string(&directory_map) {
            Ok(buff) => {
                let string_map = serde_json::from_str::<BTreeMap<String, String>>(&buff)
                    .with_context(|| format!("Failed to parse directory map: {:?}", &buff))?;
                Ok(string_map
                    .into_iter()
                    .map(|(key, val)| (key.into(), val))
                    .collect())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e)
                .with_context(|| format!("Failed to read directory map from {:?}", directory_map)),
        }
    }

    pub fn clients_dir(&self) -> PathBuf {
        self.config_dir.join(CLIENTS_DIR)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.config_dir.join("logs")
    }

    pub fn storage_dir(&self) -> PathBuf {
        self.config_dir.join("storage")
    }

    pub fn client_name(&self, path: &Path) -> Result<String> {
        // Resolve symlinks and get absolute path
        let path = path.canonicalize().from_err()?;
        #[cfg(windows)]
        let path = strip_unc_prefix(path);

        // Find `checkout_path` that `path` is a sub path of
        let all_checkouts = self.get_configured_mounts_map()?;
        if let Some(item) = all_checkouts
            .iter()
            .find(|&(checkout_path, _)| path.starts_with(checkout_path))
        {
            let (_, checkout_name) = item;
            Ok(checkout_name.clone())
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Checkout path {} is not handled by EdenFS",
                path.display()
            )))
        }
    }

    pub fn config_directory(&self, client_name: &str) -> PathBuf {
        self.clients_dir().join(client_name)
    }
}

pub trait DaemonHealthy {
    fn is_healthy(&self) -> bool;
}

impl DaemonHealthy for DaemonInfo {
    fn is_healthy(&self) -> bool {
        self.status
            .map_or_else(|| false, |val| val == fb303_status::ALIVE)
    }
}
