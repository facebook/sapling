/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use async_runtime::block_on;
use fbthrift_socket::SocketTransport;
use serde::Deserialize;
use thrift_types::edenfs;
use thrift_types::edenfs::client::EdenService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;
use types::HgId;
use types::RepoPathBuf;

use crate::types::CheckoutConflict;
use crate::types::CheckoutMode;
use crate::types::EdenError;
use crate::types::FileStatus;

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

    /// Used by thrift parameters.
    fn root_vec(&self) -> Vec<u8> {
        self.eden_config.root.clone().into_bytes()
    }

    /// Get file status. Normalized to non-Thrift types.
    pub fn get_status(
        &self,
        commit: HgId,
        list_ignored: bool,
    ) -> anyhow::Result<BTreeMap<RepoPathBuf, FileStatus>> {
        let thrift_client = block_on(self.get_thrift_client())?;
        let thrift_result = extract_error(block_on(thrift_client.getScmStatusV2(
            &edenfs::GetScmStatusParams {
                mountPoint: self.root_vec(),
                commit: commit.into_byte_array().into(),
                listIgnored: list_ignored,
                ..Default::default()
            },
        )))?;
        let mut result = BTreeMap::new();
        for (path_bytes, status) in thrift_result.status.entries {
            let path = match RepoPathBuf::from_utf8(path_bytes) {
                Err(e) => {
                    tracing::warn!("ignored non-utf8 path {}", e);
                    continue;
                }
                Ok(path) => path,
            };
            let status = status.into();
            result.insert(path, status);
        }
        Ok(result)
    }

    /// Set the working copy (dirstate) parents.
    pub fn set_parents(&self, p1: HgId, p2: Option<HgId>, p1_tree: HgId) -> anyhow::Result<()> {
        let thrift_client = block_on(self.get_thrift_client())?;
        let parents = edenfs::WorkingDirectoryParents {
            parent1: p1.into_byte_array().into(),
            parent2: p2.map(|n| n.into_byte_array().into()),
            ..Default::default()
        };
        let root_vec = self.root_vec();
        let params = edenfs::ResetParentCommitsParams {
            hgRootManifest: Some(p1_tree.into_byte_array().into()),
            ..Default::default()
        };
        extract_error(block_on(
            thrift_client.resetParentCommits(&root_vec, &parents, &params),
        ))?;
        Ok(())
    }

    /// Check out the given commit.
    /// The client might want to write pending draft changes to disk
    /// so edenfs can find the new files during checkout.
    /// Normalize to non-Thrift types.
    pub fn checkout(
        &self,
        node: HgId,
        tree: HgId,
        mode: CheckoutMode,
    ) -> anyhow::Result<Vec<CheckoutConflict>> {
        let tree_vec = tree.into_byte_array().into();
        let thrift_client = block_on(self.get_thrift_client())?;
        let params = edenfs::CheckOutRevisionParams {
            hgRootManifest: Some(tree_vec),
            ..Default::default()
        };
        let root_vec = self.root_vec();
        let node_vec = node.into_byte_array().into();
        let thrift_mode: edenfs::CheckoutMode = mode.into();
        let thrift_result = extract_error(block_on(thrift_client.checkOutRevision(
            &root_vec,
            &node_vec,
            &thrift_mode,
            &params,
        )))?;
        let result = thrift_result
            .into_iter()
            .filter_map(|c| CheckoutConflict::try_from(c).ok())
            .collect();
        Ok(result)
    }
}

/// Extract EdenError from Thrift generated enums.
/// For example, turn GetScmStatusV2Error::ex(EdenError) into this crate's EdenError.
fn extract_error<V, E: std::error::Error + Send + Sync + 'static>(
    result: std::result::Result<V, E>,
) -> anyhow::Result<V> {
    match result {
        Err(err) => {
            if let Some(source) = err.source() {
                if let Ok(err) = EdenError::try_from(source) {
                    return Err(err.into());
                }
            }
            Err(err.into())
        }
        Ok(v) => Ok(v),
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

            match util::file::read_to_string(toml_path) {
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
