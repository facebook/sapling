/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFsInstance - manages EdenFS resources besides Thrift connection (managed by
//! [`EdenFsClient`]).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context};

use edenfs_config::EdenFsConfig;
use edenfs_error::{EdenFsError, Result, ResultExt};
use fbthrift_socket::SocketTransport;
use thrift_types::edenfs::{client::EdenService, types::DaemonInfo};
use thrift_types::fb303_core::types::fb303_status;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;
use tracing::{event, Level};

use crate::utils::get_executable;
use crate::EdenFsClient;

/// These paths are relative to the user's client directory.
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

    /// Read the pid from the Eden lockfile
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

        let exe = get_executable(pid).ok_or_else(|| anyhow!("EdenFS is not running as {}", pid))?;
        let name = exe
            .file_name()
            .ok_or_else(|| anyhow!("Unable to retrieve process information of PID={}", pid))?
            .to_string_lossy();

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
    pub fn get_configured_mounts_map(&self) -> Result<BTreeMap<String, String>, anyhow::Error> {
        let directory_map = self.config_dir.join(CONFIG_JSON);
        let mut file = File::open(&directory_map)
            .with_context(|| format!("Failed to read directory map from {:?}", directory_map))?;
        let mut buff = String::new();
        file.read_to_string(&mut buff)
            .with_context(|| format!("Failed to read from {:?}", directory_map))?;
        serde_json::from_str::<BTreeMap<String, String>>(&buff)
            .with_context(|| format!("Failed to parse directory map: {:?}", &buff))
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
