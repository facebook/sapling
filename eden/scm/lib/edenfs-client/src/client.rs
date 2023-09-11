/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use fbthrift_socket::SocketTransport;
use serde::Deserialize;
use thrift_types::edenfs::client::EdenService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;

/// EdenFS client for Sapling CLI integration.
pub struct EdenFsClient {
    eden_config: EdenConfig,
}

impl EdenFsClient {
    /// Construct the client from the working directory root.
    pub fn from_wdir(wdir_root: &Path) -> anyhow::Result<Self> {
        let eden_config = EdenConfig::from_root(wdir_root)?;
        Ok(Self { eden_config })
    }

    /// Get the EdenFS root path. This is usually the working directory root.
    pub fn root(&self) -> &str {
        self.eden_config.root.as_ref()
    }

    /// Construct a raw Thrift client from the given repo root.
    pub(crate) async fn get_thrift_client(&self) -> anyhow::Result<Arc<dyn EdenService>> {
        let transport = get_socket_transport(&self.eden_config.socket).await?;
        let client = <dyn EdenService>::new(BinaryProtocol, transport);
        Ok(client)
    }
}

async fn get_socket_transport(sock_path: &Path) -> Result<SocketTransport<UnixStream>> {
    let sock = UnixStream::connect(&sock_path).await?;
    Ok(SocketTransport::new(sock))
}

#[derive(Deserialize)]
struct EdenConfig {
    root: String,
    socket: PathBuf,
}

impl EdenConfig {
    fn from_root(root: &Path) -> Result<Self> {
        let dot_eden = root.join(".eden");

        // Look up the mount point name where Eden thinks this repository is
        // located.  This may be different from repo_root if a parent directory
        // of the Eden mount has been bind mounted to another location, resulting
        // in the Eden mount appearing at multiple separate locations.

        // Windows uses a toml .eden/config file due to lack of symlink support.
        if cfg!(windows) {
            let toml_path = dot_eden.join("config");

            match util::file::read_to_string(&toml_path) {
                Ok(toml_contents) => {
                    #[derive(Deserialize)]
                    struct Outer {
                        #[serde(rename = "Config")]
                        config: EdenConfig,
                    }

                    let outer: Outer = toml::from_str(&toml_contents)?;
                    return Ok(outer.config);
                }
                // Fallthrough and try symlinks just in case.
                Err(err) if err.is_not_found() => {}
                Err(err) => return Err(err.into()),
            }
        }

        let root = util::file::read_link(dot_eden.join("root"))?
            .into_os_string()
            .into_string()
            .map_err(|path| anyhow!("couldn't stringify path {:?}", path))?;
        Ok(Self {
            root,
            socket: util::file::read_link(dot_eden.join("socket"))?,
        })
    }
}
