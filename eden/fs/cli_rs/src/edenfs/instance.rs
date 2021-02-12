/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFsInstance - manages EdenFS resources besides Thrift connection (managed by
//! [`EdenFsClient`]).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio_02::net::UnixStream;

use fbthrift_socket::SocketTransport;
use thrift_types::edenfs::client::EdenService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;

use super::utils::get_executable;
use super::EdenFsClient;

#[derive(Debug)]
pub struct EdenFsInstance {
    config_dir: PathBuf,
    etc_eden_dir: PathBuf,
    home_dir: PathBuf,
}

impl EdenFsInstance {
    pub fn new(config_dir: PathBuf, etc_eden_dir: PathBuf, home_dir: PathBuf) -> Self {
        Self {
            config_dir,
            etc_eden_dir,
            home_dir,
        }
    }

    async fn _connect(&self, socket_path: &PathBuf) -> Result<EdenFsClient> {
        let stream = UnixStream::connect(&socket_path)
            .await
            .with_context(|| format!("unable to connect to '{}'", socket_path.display()))?;
        let transport = SocketTransport::new(stream);
        let client = EdenService::new(BinaryProtocol, transport);

        Ok(client)
    }

    pub async fn connect(&self, timeout: Duration) -> Result<EdenFsClient> {
        let socket_path = self.config_dir.join("socket");

        tokio::time::timeout(timeout, self._connect(&socket_path))
            .await
            .with_context(|| {
                format!(
                    "Timed out while trying to connect to '{}'",
                    socket_path.display()
                )
            })?
            .with_context(|| format!("failed to connect to socket at {}", socket_path.display()))
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
    fn pid(&self) -> Result<sysinfo::Pid> {
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
    pub fn status_from_lock(&self) -> Result<i32> {
        let pid = self.pid()?;

        let exe = get_executable(pid).ok_or_else(|| anyhow!("EdenFS is not running as {}", pid))?;
        let name = exe
            .file_name()
            .ok_or_else(|| anyhow!("Unable to retrieve process information of PID={}", pid))?;

        if name == "edenfs" || name == "fake_edenfs" {
            Err(anyhow!(
                "EdenFS's Thrift server does not appear to be running, \
                but the process is still alive (PID={})",
                pid
            ))
        } else {
            Err(anyhow!("EdenFS is not running"))
        }
    }
}
