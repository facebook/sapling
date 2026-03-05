/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::error::Error as _;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use async_runtime::block_on;
use clientinfo::get_client_request_info;
use configmodel::Config;
use configmodel::ConfigExt;
use fbthrift_socket::SocketTransport;
use filters::filter::FilterGenerator;
use filters::id::FilterId;
use filters::migration::FilterSyncResult;
use filters::migration::cleanup_filter_sync_backup;
use filters::migration::rollback_filter_sync;
use filters::migration::sync_filters;
use parking_lot::Mutex;
use serde::Deserialize;
use thrift_types::edenfs;
use thrift_types::edenfs::CheckoutProgressInfoRequest;
use thrift_types::edenfs::CheckoutProgressInfoResponse;
use thrift_types::edenfs::RootIdOptions;
use thrift_types::edenfs_clients::EdenService;
use thrift_types::fbthrift::ApplicationException;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;
use tracing::error;
use types::HgId;
use types::RepoPathBuf;

use crate::types::CheckoutConflict;
use crate::types::CheckoutMode;
use crate::types::EdenError;
use crate::types::FileStatus;
use crate::types::LocalFrom;
use crate::types::LocalTryFrom;
use crate::types::ProgressInfo;

/// EdenFS client for Sapling CLI integration.
pub struct EdenFsClient {
    eden_config: EdenConfig,
    filter_generator: Option<Mutex<FilterGenerator>>,
    dot_dir: PathBuf,
}

impl EdenFsClient {
    /// Construct a client and FilterGenerator using the supplied working dir
    /// root. The latter is used to pass a FilterId to each thrift call.
    pub fn from_wdir(
        wdir_root: &Path,
        shared_dot_dir: &Path,
        config: Arc<dyn Config>,
    ) -> anyhow::Result<Self> {
        let dot_dir = wdir_root.join(identity::must_sniff_dir(wdir_root)?.dot_dir());
        let eden_config = EdenConfig::from_root(wdir_root)?;
        let filter_generator = FilterGenerator::from_dot_dirs(&dot_dir, shared_dot_dir, &config)?;
        Ok(Self {
            eden_config,
            filter_generator: Some(Mutex::new(filter_generator)),
            dot_dir,
        })
    }

    pub fn get_active_filter_id(&self, commit: HgId) -> Result<Option<FilterId>, anyhow::Error> {
        match &self.filter_generator {
            Some(r#gen) => {
                let mut lock = r#gen.lock();
                lock.active_filter_id(commit)
            }
            None => Ok(None),
        }
    }

    /// Get the EdenFS root path. This is usually the working directory root.
    pub fn root(&self) -> &str {
        self.eden_config.root.as_ref()
    }

    /// Get the EdenFS "client" path. This is different from the "root" path.
    /// The client path contains files like `config.toml`.
    pub fn client_path(&self) -> &Path {
        self.eden_config.client.as_ref()
    }

    /// Construct a raw Thrift client from the given repo root.
    pub(crate) async fn get_async_thrift_client(&self) -> anyhow::Result<Arc<dyn EdenService>> {
        let transport = get_socket_transport(&self.eden_config.socket).await?;
        let client = <dyn EdenService>::new(BinaryProtocol, transport);
        Ok(client)
    }

    /// Construct a RequestInfo to pass alone with requests.
    pub(crate) fn get_client_request_info(&self) -> edenfs::ClientRequestInfo {
        let slcri = get_client_request_info();
        edenfs::ClientRequestInfo {
            correlator: slcri.correlator,
            entry_point: slcri.entry_point.to_string(),
            ..Default::default()
        }
    }

    /// Used by thrift parameters.
    fn root_vec(&self) -> Vec<u8> {
        self.eden_config.root.clone().into_bytes()
    }

    fn root_options_from_filter(filter: Option<FilterId>) -> RootIdOptions {
        let fid = filter.and_then(|filt| filt.id().ok());
        edenfs::RootIdOptions {
            fid,
            ..Default::default()
        }
    }

    /// Get file status. Normalized to non-Thrift types.
    #[tracing::instrument(skip(self))]
    pub fn get_status(
        &self,
        commit: HgId,
        list_ignored: bool,
    ) -> anyhow::Result<BTreeMap<RepoPathBuf, FileStatus>> {
        let thrift_client = block_on(self.get_async_thrift_client())?;

        let start_time = Instant::now();
        let root_id_options = Self::root_options_from_filter(self.get_active_filter_id(commit)?);
        let thrift_result = extract_error(block_on(thrift_client.getScmStatusV2(
            &edenfs::GetScmStatusParams {
                mountPoint: self.root_vec(),
                commit: commit.into_byte_array().into(),
                listIgnored: list_ignored,
                cri: Some(self.get_client_request_info()),
                rootIdOptions: Some(root_id_options),
                ..Default::default()
            },
        )))?;

        hg_metrics::increment_counter(
            "edenclientstatus_time",
            start_time.elapsed().as_millis() as u64,
        );

        hg_metrics::max_counter(
            "edenclientstatus_length",
            thrift_result.status.entries.len() as u64,
        );

        tracing::debug!(target: "eden_info", eden_version=thrift_result.version);

        let mut result = BTreeMap::new();
        for (path_bytes, status) in thrift_result.status.entries {
            let path = match RepoPathBuf::from_utf8(path_bytes) {
                Err(e) => {
                    tracing::warn!("ignored non-utf8 path {}", e);
                    continue;
                }
                Ok(path) => path,
            };
            let status = FileStatus::local_from(status);
            result.insert(path, status);
        }
        Ok(result)
    }

    /// Get the raw journal position. Useful to check whether there are file changes.
    #[tracing::instrument(skip(self))]
    pub fn get_journal_position(&self) -> anyhow::Result<(i64, i64)> {
        let thrift_client = block_on(self.get_async_thrift_client())?;
        let position = extract_error(block_on(
            thrift_client.getCurrentJournalPosition(&self.root_vec()),
        ))?;
        let position = (position.mountGeneration, position.sequenceNumber);
        tracing::debug!("journal position {:?}", position);
        Ok(position)
    }

    /// Like get_journal_position but doesn't mark the journal as observed.
    /// Falls back to get_journal_position if the server doesn't support this method.
    #[tracing::instrument(skip(self))]
    pub fn peek_journal_position(&self) -> anyhow::Result<(i64, i64)> {
        let thrift_client = block_on(self.get_async_thrift_client())?;
        let result = block_on(thrift_client.peekCurrentJournalPosition(
            &edenfs::PeekCurrentJournalPositionRequest {
                mountId: edenfs::MountId {
                    mountPoint: self.root_vec(),
                    ..Default::default()
                },
                ..Default::default()
            },
        ));

        if let Err(err) = &result {
            if let Some(app_ex) = err
                .source()
                .and_then(|s| s.downcast_ref::<ApplicationException>())
            {
                // TODO: remove fallback once peekCurrentJournalPosition is available everywhere
                if app_ex.type_ == ApplicationExceptionErrorCode::UnknownMethod {
                    tracing::debug!("peekCurrentJournalPosition not available, falling back");
                    return self.get_journal_position();
                }
            }
        }

        let response = extract_error(result)?;
        let position = (
            response.position.mountGeneration,
            response.position.sequenceNumber,
        );
        tracing::debug!("journal position {:?}", position);
        Ok(position)
    }

    /// Set the working copy (dirstate) parents.
    #[tracing::instrument(skip(self))]
    pub fn set_parents(&self, p1: HgId, p2: Option<HgId>, p1_tree: HgId) -> anyhow::Result<()> {
        let thrift_client = block_on(self.get_async_thrift_client())?;
        let parents = edenfs::WorkingDirectoryParents {
            parent1: p1.into_byte_array().into(),
            parent2: p2.map(|n| n.into_byte_array().into()),
            ..Default::default()
        };

        let root_id_options = Self::root_options_from_filter(self.get_active_filter_id(p1)?);
        let root_vec = self.root_vec();
        let params = edenfs::ResetParentCommitsParams {
            hgRootManifest: Some(p1_tree.into_byte_array().into()),
            cri: Some(self.get_client_request_info()),
            rootIdOptions: Some(root_id_options),
            ..Default::default()
        };
        extract_error(block_on(
            thrift_client.resetParentCommits(&root_vec, &parents, &params),
        ))?;
        Ok(())
    }

    /// Returns the current progress checkout counter(s) for the current mount.
    /// When a checkout is not ongoing it returns None.
    #[tracing::instrument(skip(self))]
    pub fn checkout_progress(&self) -> anyhow::Result<Option<ProgressInfo>> {
        let thrift_client = block_on(self.get_async_thrift_client())?;
        let root_vec = self.root_vec();
        let thrift_params = CheckoutProgressInfoRequest {
            mountPoint: root_vec,
            ..Default::default()
        };
        let thrift_result = extract_error(block_on(
            thrift_client.getCheckoutProgressInfo(&thrift_params),
        ))?;
        Ok(
            if let CheckoutProgressInfoResponse::checkoutProgressInfo(info) = thrift_result {
                Some(ProgressInfo {
                    position: info.updatedInodes as u64,
                    total: info.totalInodes as u64,
                })
            } else {
                None
            },
        )
    }

    /// Check out the given commit.
    /// The client might want to write pending draft changes to disk
    /// so edenfs can find the new files during checkout.
    /// Normalize to non-Thrift types.
    #[tracing::instrument(skip(self, config))]
    pub fn checkout(
        &self,
        config: &dyn Config,
        node: HgId,
        tree: HgId,
        mode: CheckoutMode,
    ) -> anyhow::Result<Vec<CheckoutConflict>> {
        let disable_filter_sync: bool =
            config.get_or_default("edensparse", "disable-filter-sync")?;
        let sync_result: Option<FilterSyncResult> =
            if !disable_filter_sync && matches!(mode, CheckoutMode::Force | CheckoutMode::Normal) {
                Some(sync_filters(&self.dot_dir, config)?)
            } else {
                None
            };

        if let Some(FilterSyncResult::Updated {
            ref previous,
            ref current,
        }) = sync_result
        {
            tracing::info!(
                "Filter sync: {:?} -> {:?}",
                previous.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                current.iter().map(|p| p.as_str()).collect::<Vec<_>>()
            );
        }

        let tree_bytes: Vec<u8> = tree.into_byte_array().into();
        let thrift_client = block_on(self.get_async_thrift_client())?;

        let root_id_options = Self::root_options_from_filter(self.get_active_filter_id(node)?);
        let params = edenfs::CheckOutRevisionParams {
            hgRootManifest: Some(tree_bytes.clone()),
            cri: Some(self.get_client_request_info()),
            rootIdOptions: Some(root_id_options),
            ..Default::default()
        };
        let root_vec = self.root_vec();
        let node_vec: Vec<u8> = node.into_byte_array().into();
        let thrift_mode = edenfs::CheckoutMode::local_from(mode);

        let start_time = Instant::now();

        let mut thrift_result = extract_error(block_on(thrift_client.checkOutRevision(
            &root_vec,
            &node_vec,
            &thrift_mode,
            &params,
        )));

        match (&sync_result, &thrift_result) {
            (Some(FilterSyncResult::Updated { .. }), Err(err)) => {
                // Checkout failed, rollback filter changes so that EdenFS and
                // Sapling sources of truth match
                rollback_filter_sync(&self.dot_dir)?;

                // If the error is CHECKOUT_IN_PROGRESS for the same commit,
                // the filter sync changed the filter ID causing a mismatch
                // with the interrupted checkout's stored destination. Now that
                // we've rolled back .hg/sparse, retry with the original
                // filter ID.
                if is_interrupted_checkout_error(err, &node) {
                    let root_id_options =
                        Self::root_options_from_filter(self.get_active_filter_id(node)?);
                    let retry_params = edenfs::CheckOutRevisionParams {
                        hgRootManifest: Some(tree_bytes),
                        cri: Some(self.get_client_request_info()),
                        rootIdOptions: Some(root_id_options),
                        ..Default::default()
                    };
                    let retry_result = extract_error(block_on(thrift_client.checkOutRevision(
                        &root_vec,
                        &node_vec,
                        &thrift_mode,
                        &retry_params,
                    )));
                    if retry_result.is_ok() {
                        cleanup_filter_sync_backup(&self.dot_dir)?;
                    }
                    thrift_result = retry_result;
                }
            }
            (Some(_), Ok(_)) => {
                // Checkout succeeded, cleanup filter backup and legacy migration files
                cleanup_filter_sync_backup(&self.dot_dir)?;
            }
            _ => {}
        }

        hg_metrics::increment_counter(
            "edenclientcheckout_time",
            start_time.elapsed().as_millis() as u64,
        );

        let result = thrift_result?
            .into_iter()
            .filter_map(|c| CheckoutConflict::local_try_from(c).ok())
            .collect::<Vec<_>>();
        hg_metrics::increment_counter("eden_conflict_count", result.len() as u64);
        Ok(result)
    }
}

/// Extract EdenError from Thrift generated enums.
/// For example, turn GetScmStatusV2Error::ex(EdenError) into this crate's EdenError.
pub(crate) fn extract_error<V, E: std::error::Error + Send + Sync + 'static>(
    result: std::result::Result<V, E>,
) -> anyhow::Result<V> {
    match result {
        Err(err) => {
            if let Some(source) = err.source() {
                if let Ok(err) = EdenError::local_try_from(source) {
                    return Err(err.into());
                }
            }
            Err(err.into())
        }
        Ok(v) => Ok(v),
    }
}

/// Check if an error is an EdenFS CHECKOUT_IN_PROGRESS error for a specific commit.
fn is_interrupted_checkout_error(err: &anyhow::Error, node: &HgId) -> bool {
    if let Some(eden_err) = err.downcast_ref::<EdenError>() {
        return eden_err.error_type == "CHECKOUT_IN_PROGRESS"
            && eden_err.message.contains(&node.to_hex());
    }
    false
}

async fn get_socket_transport(sock_path: &Path) -> Result<SocketTransport<UnixStream>> {
    let sock = UnixStream::connect(&sock_path).await?;
    Ok(SocketTransport::new_with_error_handler(
        sock,
        |error| error!(target: "transport_errors", thrift_transport_error=?error),
    ))
}

#[derive(Deserialize)]
struct EdenConfig {
    root: String,
    socket: PathBuf,
    client: PathBuf,
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

            match fs_err::read_to_string(toml_path) {
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
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }

        let root = fs_err::read_link(dot_eden.join("root"))?
            .into_os_string()
            .into_string()
            .map_err(|path| anyhow!("couldn't stringify path {:?}", path))?;
        let socket = fs_err::read_link(dot_eden.join("socket"))?;
        let client = fs_err::read_link(dot_eden.join("client"))?;
        Ok(Self {
            root,
            socket,
            client,
        })
    }
}
