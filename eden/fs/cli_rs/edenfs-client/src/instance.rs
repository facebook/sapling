/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFsInstance - manages EdenFS resources besides Thrift connection (managed by
//! [`EdenFsThriftClient`]).

use std::collections::BTreeMap;
#[cfg(windows)]
use std::fs::remove_file;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use atomicfile::atomic_write;
use edenfs_config::EdenFsConfig;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use edenfs_utils::get_executable;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use futures::stream::BoxStream;
use futures::StreamExt;
use thrift_streaming_clients::errors::SubscribeStreamTemporaryError;
#[cfg(fbcode_build)]
use thrift_types::edenfs::DaemonInfo;
use thrift_types::edenfs::GetConfigParams;
use thrift_types::edenfs::GetCurrentSnapshotInfoRequest;
use thrift_types::edenfs::GetScmStatusParams;
use thrift_types::edenfs::GlobParams;
use thrift_types::edenfs::MountId;
#[cfg(target_os = "macos")]
use thrift_types::edenfs::StartFileAccessMonitorParams;
use thrift_types::edenfs::UnmountArgument;
use thrift_types::edenfs_clients::errors::UnmountV2Error;
use thrift_types::fb303_core::fb303_status;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use tokio::time;
use tracing::event;
use tracing::Level;
use util::lock::PathLock;

use crate::changes_since::ChangesSinceV2Result;
use crate::client::EdenFsClient;
use crate::client::StreamingEdenFsClient;
use crate::journal_position::JournalPosition;
use crate::utils::get_mount_point;
use crate::EdenFsThriftClient;

// We should create a single EdenFsInstance when parsing EdenFs commands and utilize
// EdenFsInstance::global() whenever we need to access it. This way we can avoid passing an
// EdenFsInstance through every subcommand
static INSTANCE: OnceLock<EdenFsInstance> = OnceLock::new();

// Default config and etc dirs
#[cfg(unix)]
pub const DEFAULT_CONFIG_DIR: &str = "~/local/.eden";
#[cfg(unix)]
pub const DEFAULT_ETC_EDEN_DIR: &str = "/etc/eden";

#[cfg(windows)]
pub const DEFAULT_CONFIG_DIR: &str = "~\\.eden";
#[cfg(windows)]
pub const DEFAULT_ETC_EDEN_DIR: &str = "C:\\ProgramData\\facebook\\eden";

/// These paths are relative to the user's client directory.
const CLIENTS_DIR: &str = "clients";
const CONFIG_JSON: &str = "config.json";
const CONFIG_JSON_LOCK: &str = "config.json.lock";
const CONFIG_JSON_MODE: u32 = 0o664;

#[derive(Debug)]
pub struct EdenFsInstance {
    config_dir: PathBuf,
    etc_eden_dir: PathBuf,
    home_dir: Option<PathBuf>,
}

impl EdenFsInstance {
    pub fn global() -> &'static EdenFsInstance {
        INSTANCE.get().expect("EdenFsInstance is not initialized")
    }

    pub fn new(
        config_dir: PathBuf,
        etc_eden_dir: PathBuf,
        home_dir: Option<PathBuf>,
    ) -> EdenFsInstance {
        Self {
            config_dir,
            etc_eden_dir,
            home_dir,
        }
    }

    pub fn init(config_dir: PathBuf, etc_eden_dir: PathBuf, home_dir: Option<PathBuf>) {
        event!(
            Level::TRACE,
            ?config_dir,
            ?etc_eden_dir,
            ?home_dir,
            "Creating EdenFsInstance"
        );
        INSTANCE
            .set(EdenFsInstance::new(config_dir, etc_eden_dir, home_dir))
            .expect("should be able to initialize EdenfsInstance")
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

    pub async fn get_client(&self, timeout: Option<Duration>) -> Result<EdenFsClient> {
        EdenFsClient::new(self, timeout).await
    }

    pub async fn get_streaming_client(
        &self,
        timeout: Option<Duration>,
    ) -> Result<StreamingEdenFsClient> {
        StreamingEdenFsClient::new(self, timeout).await
    }

    pub(crate) fn socketfile(&self) -> PathBuf {
        self.config_dir.join("socket")
    }

    /// Returns the path to the EdenFS socket file. If check is true, it will check if the socket
    /// file exists or not. If it doesn't exist, it will return an error.
    pub fn get_socket_path(&self, check: bool) -> Result<PathBuf, anyhow::Error> {
        let socketfile = self.socketfile();

        if check {
            if !std::fs::exists(&socketfile).with_context(|| {
                format!(
                    "Failed to check existence of socket file {}",
                    socketfile.display()
                )
            })? {
                return Err(anyhow!(
                    "EdenFS socket file {} doesn't exist on this machine",
                    socketfile.display()
                ));
            }
        }
        Ok(socketfile.to_owned())
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

    pub async fn stream_journal_changed(
        &self,
        mount_point: &Option<PathBuf>,
    ) -> Result<
        BoxStream<
            'static,
            Result<thrift_types::edenfs::JournalPosition, SubscribeStreamTemporaryError>,
        >,
        EdenFsError,
    > {
        let mount_point_vec = bytes_from_path(get_mount_point(mount_point)?)?;
        let stream_client = self
            .get_streaming_client(None)
            .await
            .with_context(|| anyhow!("unable to establish Thrift connection to EdenFS server"))?;
        let stream_client = stream_client.get_thrift_client();

        stream_client
            .streamJournalChanged(&mount_point_vec)
            .await
            .from_err()
    }

    pub async fn subscribe(
        &self,
        mount_point: &Option<PathBuf>,
        throttle_time_ms: u64,
        position: Option<JournalPosition>,
        root: &Option<PathBuf>,
        included_roots: &Option<Vec<PathBuf>>,
        included_suffixes: &Option<Vec<String>>,
        excluded_roots: &Option<Vec<PathBuf>>,
        excluded_suffixes: &Option<Vec<String>>,
        include_vcs_roots: bool,
        handle_results: impl Fn(&ChangesSinceV2Result) -> Result<(), EdenFsError>,
    ) -> Result<(), anyhow::Error> {
        let client = self.get_client(None).await?;
        let mut position = position.unwrap_or(client.get_journal_position(mount_point).await?);
        let mut subscription = self.stream_journal_changed(mount_point).await?;

        let mut last = Instant::now();
        let throttle = Duration::from_millis(throttle_time_ms);

        let mut pending_updates = false;

        // Largest allowed sleep value  https://docs.rs/tokio/latest/tokio/time/fn.sleep.html
        let sleep_max = Duration::from_millis(68719476734);
        let timer = time::sleep(sleep_max);
        tokio::pin!(timer);

        loop {
            tokio::select! {
                // Wait on the following cases
                // 1. The we get a notification from the subscription
                // 2. The pending updates timer expires
                // 3. Another signal is received
                result = subscription.next() => {
                    match result {
                        // if the stream is ended somehow, we terminate as well
                        None => break,
                        // if any error happened during the stream, log them
                        Some(Err(e)) => {
                            tracing::error!(?e, "error while processing subscription");
                            continue;
                        },
                        // If we have recently(within throttle ms) sent an update, set a
                        // timer to check again when throttle time is up if we aren't already
                        // waiting on a timer
                        Some(Ok(_)) => {
                            if last.elapsed() < throttle && !pending_updates {
                                // set timer to check again when throttle time is up
                                pending_updates = true;
                                timer.as_mut().reset((Instant::now() + throttle).into());
                                continue;
                            }
                        }
                    }
                },
                // Pending updates timer expired. If we haven't gotten a subscription notification in
                // the meantime, check for updates now. Set the timer back to the max value in either case.
                () = &mut timer => {
                    // Set timer to the maximum value to prevent repeated wakeups since timers are not consumed
                    timer.as_mut().reset((Instant::now() + sleep_max).into());
                    if !pending_updates {
                        continue;
                    }
                },
                // in all other cases, we terminate
                else => break,
            }

            let result = client
                .get_changes_since(
                    mount_point,
                    &position,
                    root,
                    included_roots,
                    included_suffixes,
                    excluded_roots,
                    excluded_suffixes,
                    include_vcs_roots,
                    None,
                )
                .await
                .map_err(anyhow::Error::msg)?;

            tracing::debug!(
                "got {} changes for position {}",
                result.changes.len(),
                result.to_position
            );

            if !result.changes.is_empty() {
                // Error in handle results will terminate the loop
                handle_results(&result)?;
            }

            pending_updates = false;
            position = result.to_position;

            last = Instant::now();
        }

        Ok(())
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

    pub fn client_dir_for_mount_point(&self, path: &Path) -> Result<PathBuf> {
        Ok(self.clients_dir().join(self.client_name(path)?))
    }

    pub fn remove_path_from_directory_map(&self, path: &Path) -> Result<()> {
        let lock_file_path = self.config_dir.join(CONFIG_JSON_LOCK);
        let config_file_path = self.config_dir.join(CONFIG_JSON);

        // For Linux and MacOS we have a lock file "config.json.lock" under the config directory
        // which works as a file lock to prevent the file "config.json" being accessed by
        // multiple processes at the same time.
        //
        // In Python CLI code, FileLock lib is used to create config.json.lock.
        // In Rust, we use PathLock from "scm/lib/util"
        let _lock = PathLock::exclusive(&lock_file_path).with_context(|| {
            format!("Failed to open the lock file {}", lock_file_path.display())
        })?;

        // Lock acquired, now we can read and write to the "config.json" file

        // On Windows the "Path" crate will append the prefix "\\?\" to the original path when
        // "canonicalize()" is called to indicate the path is in unicode.
        // We need to strip the prefix before checking the key in "config.json" file
        // For non-windows platforms, this is no-op.
        let entry_key = dunce::simplified(path);
        let mut all_checkout_map = self.get_configured_mounts_map()?;
        let original_num_of_entries = all_checkout_map.len();

        all_checkout_map.retain(|path, _| dunce::simplified(path) != entry_key);

        if all_checkout_map.len() < original_num_of_entries {
            atomic_write(&config_file_path, CONFIG_JSON_MODE, true, |f| {
                serde_json::to_writer_pretty(f, &all_checkout_map)?;
                Ok(())
            })
            .with_context(|| {
                format!(
                    "Failed to write updated config JSON back to {}",
                    config_file_path.display()
                )
            })?;
        } else {
            event!(
                Level::WARN,
                "There is not entry for {} in config.json",
                path.display()
            );
        }

        // Lock will be released when _lock is dropped
        Ok(())
    }

    pub async fn unmount(&self, path: &Path, no_force: bool) -> Result<()> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        let encoded_path = bytes_from_path(path.to_path_buf())
            .with_context(|| format!("Failed to encode path {}", path.display()))?;

        let unmount_argument = UnmountArgument {
            mountId: MountId {
                mountPoint: encoded_path,
                ..Default::default()
            },
            useForce: !no_force,
            ..Default::default()
        };
        match client.unmountV2(&unmount_argument).await {
            Ok(_) => Ok(()),
            Err(UnmountV2Error::ApplicationException(ref e)) => {
                if e.type_ == ApplicationExceptionErrorCode::UnknownMethod {
                    let encoded_path = bytes_from_path(path.to_path_buf())
                        .with_context(|| format!("Failed to encode path {}", path.display()))?;
                    client.unmount(&encoded_path).await.with_context(|| {
                        format!(
                            "Failed to unmount (legacy Thrift unmount endpoint) {}",
                            path.display()
                        )
                    })?;
                    Ok(())
                } else {
                    Err(EdenFsError::Other(anyhow!(
                        "Failed to unmount (Thrift unmountV2 endpoint) {}: {}",
                        path.display(),
                        e
                    )))
                }
            }
            Err(e) => Err(EdenFsError::Other(anyhow!(
                "Failed to unmount (Thrift unmountV2 endpoint) {}: {}",
                path.display(),
                e
            ))),
        }
    }

    pub async fn get_current_snapshot_info(
        &self,
        mount_point: PathBuf,
    ) -> Result<thrift_types::edenfs::GetCurrentSnapshotInfoResponse> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();
        let mount_point = bytes_from_path(mount_point)?;
        let snapshot_info_params = GetCurrentSnapshotInfoRequest {
            mountId: MountId {
                mountPoint: mount_point,
                ..Default::default()
            },
            cri: None,
            ..Default::default()
        };

        client
            .getCurrentSnapshotInfo(&snapshot_info_params)
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get snapshot info")))
    }

    pub async fn get_scm_status_v2(
        &self,
        mount_point: PathBuf,
        commit_str: String,
        list_ignored: bool,
        root_id_options: Option<thrift_types::edenfs::RootIdOptions>,
    ) -> Result<thrift_types::edenfs::GetScmStatusResult> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .getScmStatusV2(&GetScmStatusParams {
                mountPoint: bytes_from_path(mount_point)?,
                commit: commit_str.as_bytes().to_vec(),
                listIgnored: list_ignored,
                rootIdOptions: root_id_options,
                ..Default::default()
            })
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get scm status v2 result")))
    }

    pub async fn glob_files<P: AsRef<Path>, S: AsRef<Path>>(
        &self,
        mount_point: P,
        globs: Vec<String>,
        include_dotfiles: bool,
        prefetch_files: bool,
        suppress_file_list: bool,
        want_dtype: bool,
        search_root: S,
        background: bool,
        list_only_files: bool,
    ) -> Result<thrift_types::edenfs::Glob> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .globFiles(&GlobParams {
                mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
                globs,
                includeDotfiles: include_dotfiles,
                prefetchFiles: prefetch_files,
                suppressFileList: suppress_file_list,
                wantDtype: want_dtype,
                searchRoot: bytes_from_path(search_root.as_ref().to_path_buf())?,
                background,
                listOnlyFiles: list_only_files,
                ..Default::default()
            })
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get glob files result")))
    }

    #[cfg(target_os = "linux")]
    pub async fn add_bind_mount(
        &self,
        mount_path: &Path,
        repo_path: &Path,
        target_path: &Path,
    ) -> Result<()> {
        let mount_path = bytes_from_path(mount_path.to_path_buf()).with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?;

        let repo_path = bytes_from_path(repo_path.to_path_buf()).with_context(|| {
            format!("Failed to get repo point '{}' as str", repo_path.display())
        })?;

        let target_path = bytes_from_path(target_path.to_path_buf())
            .with_context(|| format!("Failed to get target '{}' as str", target_path.display()))?;

        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .addBindMount(&mount_path, &repo_path, &target_path)
            .await
            .with_context(|| "failed add bind mount thrift call")?;

        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub async fn remove_bind_mount(&self, mount_path: &Path, repo_path: &Path) -> Result<()> {
        let mount_path = bytes_from_path(mount_path.to_path_buf()).with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?;

        let repo_path = bytes_from_path(repo_path.to_path_buf()).with_context(|| {
            format!("Failed to get repo point '{}' as str", repo_path.display())
        })?;

        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .removeBindMount(&mount_path, &repo_path)
            .await
            .with_context(|| "failed remove bind mount thrift call")?;

        Ok(())
    }

    pub async fn get_config_default(&self) -> Result<thrift_types::edenfs_config::EdenConfigData> {
        let client = self
            .get_client(None)
            .await
            .with_context(|| "Unable to connect to EdenFS daemon")?;
        let client = client.get_thrift_client();

        let params: GetConfigParams = Default::default();
        client
            .getConfig(&params)
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get default eden config data")))
    }

    pub async fn stop_recording_backing_store_fetch(
        &self,
    ) -> Result<thrift_types::edenfs::GetFetchedFilesResult> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        let files = client
            .stopRecordingBackingStoreFetch()
            .await
            .with_context(|| anyhow!("stopRecordingBackingStoreFetch thrift call failed"))?;
        Ok(files)
    }

    pub async fn start_recording_backing_store_fetch(&self) -> Result<()> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .startRecordingBackingStoreFetch()
            .await
            .with_context(|| anyhow!("startRecordingBackingStoreFetch thrift call failed"))?;
        Ok(())
    }

    pub async fn debug_clear_local_store_caches(&self) -> Result<()> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .debugClearLocalStoreCaches()
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to call debugClearLocalStoreCaches")))
    }

    pub async fn debug_compact_local_storage(&self) -> Result<()> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .debugCompactLocalStorage()
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to call debugCompactLocalStorage")))
    }

    pub async fn clear_and_compact_local_store(&self) -> Result<()> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .clearAndCompactLocalStore()
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to call clearAndCompactLocalStore")))
    }

    pub async fn flush_stats_now(&self, client: &EdenFsThriftClient) -> Result<()> {
        client
            .flushStatsNow()
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to call flushstatsNow")))
    }

    pub async fn get_regex_counters(
        &self,
        arg_regex: &str,
        client: &EdenFsThriftClient,
    ) -> Result<BTreeMap<String, i64>> {
        client
            .getRegexCounters(arg_regex)
            .await
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get regex counters")))
    }

    #[cfg(target_os = "macos")]
    pub async fn start_file_access_monitor(
        &self,
        path_prefix: &Vec<PathBuf>,
        specified_output_file: Option<PathBuf>,
        should_upload: bool,
    ) -> Result<thrift_types::edenfs::StartFileAccessMonitorResult> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        let mut paths = Vec::new();
        for path in path_prefix {
            let path = bytes_from_path(path.to_path_buf())?;
            paths.push(path);
        }
        client
            .startFileAccessMonitor(&StartFileAccessMonitorParams {
                paths,
                specifiedOutputPath: match specified_output_file {
                    Some(path) => Some(bytes_from_path(path.to_path_buf())?),
                    None => None,
                },
                shouldUpload: should_upload,
                ..Default::default()
            })
            .await
            .map_err(|e| EdenFsError::Other(anyhow!("failed to start file access monitor: {}", e)))
    }

    #[cfg(target_os = "macos")]
    pub async fn stop_file_access_monitor(
        &self,
    ) -> Result<thrift_types::edenfs::StopFileAccessMonitorResult> {
        let client = self.get_client(None).await?;
        let client = client.get_thrift_client();

        client
            .stopFileAccessMonitor()
            .await
            .map_err(|e| EdenFsError::Other(anyhow!("failed to stop file access monitor: {}", e)))
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
