/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use anyhow::Result;
use anyhow::anyhow;
use arc_swap::ArcSwap;
use blob::Blob;
use configloader::Config;
use configloader::hg::PinnedConfig;
use configloader::hg::RepoInfo;
use edenapi::BlockingResponse;
use edenapi::RECENT_DOGFOODING_REQUESTS;
use edenapi::configmodel::ConfigExt;
use edenapi::configmodel::config::ContentHash;
use edenapi::types::CommitId;
use log::warn;
use metrics::ods;
use parking_lot::RwLock;
use repo::RepoMinimalInfo;
use repo::repo::Repo;
use storemodel::BoxIterator;
use storemodel::FileAuxData;
use storemodel::FileStore;
use storemodel::TreeAuxData;
use storemodel::TreeEntry;
use storemodel::TreeStore;
use tracing::instrument;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::prefetch;
use crate::prefetch::prefetch_manager;

pub struct BackingStore {
    // ArcSwap is similar to RwLock, but has lower overhead for read operations.
    inner: ArcSwap<Inner>,

    parent_hint: Arc<RwLock<Option<String>>>,
}

struct Inner {
    filestore: Arc<dyn FileStore>,
    treestore: Arc<dyn TreeStore>,
    repo: Arc<Repo>,
    mount_path: PathBuf,

    // We store these so we can maintain them when reloading ourself.
    extra_configs: Vec<PinnedConfig>,

    // State used to track the touch file and determine if we need to reload ourself.
    create_time: Instant,
    touch_file_mtime: Option<SystemTime>,

    // To prevent multiple threads reloading at the same time.
    already_reloading: AtomicBool,

    // Last time we did a full reload of the Repo.
    last_reload: Instant,

    // Controlled by config "backingstore.reload-check-interval-secs".
    // Sets the minimum delay before we check if we need to reload (defaults to 5s).
    reload_check_interval: Duration,

    // Controlled by config "backingstore.reload-interval-secs".
    // Sets the maximum time since last reload until we force a reload (defaults to 5m).
    reload_interval: Duration,

    prefetch_send: flume::Sender<()>,
    walk_mode: WalkMode,
    walk_detector: walkdetector::Detector,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum WalkMode {
    // Don't observe walks.
    Off,
    // Watch for walks, but don't take any action.
    Monitor,
    // Prefetch files/trees based on observed walks.
    Prefetch,
}

impl FromStr for WalkMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "monitor" => Ok(Self::Monitor),
            "prefetch" => Ok(Self::Prefetch),
            _ => Ok(Self::Off),
        }
    }
}

impl BackingStore {
    /// Initialize `BackingStore`.
    pub fn new<P: AsRef<Path>>(root: P, mount: P) -> Result<Self> {
        Self::new_with_config(root.as_ref(), mount.as_ref(), &[])
    }

    pub fn name(&self) -> Result<String> {
        match self.maybe_reload().repo.repo_name() {
            Some(repo_name) => Ok(repo_name.to_string()),
            None => Err(anyhow!("no repo name")),
        }
    }

    /// Initialize `BackingStore` with extra configs.
    /// This is used by benches/ to set cache path to control warm/code test cases.
    pub fn new_with_config(
        root: impl AsRef<Path>,
        mount: impl AsRef<Path>,
        extra_configs: &[String],
    ) -> Result<Self> {
        let extra_configs = extra_configs
            .iter()
            .map(|c| PinnedConfig::Raw(c.to_string().into(), "backingstore".into()))
            .collect::<Vec<_>>();

        let parent_hint = Arc::new(RwLock::default());

        Ok(Self {
            inner: ArcSwap::new(Arc::new(Self::new_inner(
                root.as_ref(),
                mount.as_ref(),
                &extra_configs,
                touch_file_mtime(),
                parent_hint.clone(),
                walkdetector::Detector::new(),
            )?)),
            parent_hint,
        })
    }

    fn new_inner(
        root: &Path,
        mount: &Path,
        extra_configs: &[PinnedConfig],
        touch_file_mtime: Option<SystemTime>,
        parent_hint: Arc<RwLock<Option<String>>>,
        mut walk_detector: walkdetector::Detector,
    ) -> Result<Inner> {
        constructors::init();

        let info = RepoMinimalInfo::from_repo_root(root.to_path_buf())?;
        let mut config = configloader::hg::embedded_load(RepoInfo::Disk(&info), extra_configs)?;

        // Allow overrideing scmstore.tree-metadata-mode for eden only.
        if let Some(mode) = config.get_nonempty("eden", "tree-metadata-mode") {
            config.set(
                "scmstore",
                "tree-metadata-mode",
                Some(mode),
                &"backingstore".into(),
            );
        }

        #[cfg(feature = "scuba")]
        edenfs_telemetry::tracing_logger::set_logged_targets(
            config
                .get_or::<Vec<String>>("backingstore", "logged-tracing-targets", || {
                    vec!["big_walk".to_string()]
                })?
                .into_iter()
                .collect(),
        );

        // Apply indexed log configs, which can affect edenfs behavior.
        indexedlog::config::configure(&config)?;

        let repo = Repo::load_with_config(root, config)?;
        let filestore = repo.file_store()?;
        let treestore = repo.tree_store()?;

        let config = repo.config().clone();

        let is_obc_enabled = config.get_or::<bool>("scmstore", "enable-obc", || false)?;
        if is_obc_enabled {
            if let Err(err) = ods::initialize_obc_client() {
                tracing::warn!(?err, "error creating OBC client");
            }
        }

        let walk_mode = WalkMode::from_str(
            config
                .get("backingstore", "walk-mode")
                .unwrap_or_default()
                .as_ref(),
        )?;

        let repo = Arc::new(repo);

        // First reset to default config values to handle the case when a config item was specified
        // in the sl config, but is no longer present (i.e. need to revert to the in-code default).
        walk_detector.reset_config();

        if let Some(threshold) = config.get_opt("backingstore", "walk-threshold")? {
            walk_detector.set_walk_threshold(threshold);
        }

        if let Some(depth) = config.get_opt("backingstore", "walk-lax-depth")? {
            walk_detector.set_lax_depth(depth);
        }

        if let Some(depth) = config.get_opt("backingstore", "walk-strict-multiplier")? {
            walk_detector.set_strict_multiplier(depth);
        }

        if let Some(threshold) = config.get_opt("backingstore", "walk-ratio")? {
            walk_detector.set_walk_ratio(threshold);
        }

        if let Some(threshold) = config.get_opt("backingstore", "walk-gc-interval")? {
            walk_detector.set_gc_interval(threshold);
        }

        if let Some(timeout) = config.get_opt("backingstore", "walk-gc-timeout")? {
            walk_detector.set_gc_timeout(timeout);
        }

        walk_detector.set_root(Some(mount.to_path_buf()));

        let prefetch_send = if walk_mode == WalkMode::Prefetch {
            let prefetch_config = prefetch::Config {
                // Pause prefetch if ratio of cache hits to prefetches is below min_ratio AND
                // prefetches - cache hits is greater than max_initial_lag.
                min_ratio: config.get_or("backingstore", "walk-prefetch-min-ratio", || 0.1)?,
                max_initial_lag: config.get_or(
                    "backingstore",
                    "walk-prefetch-max-initial-lag",
                    || 50_000,
                )?,
                min_interval: config.get_or(
                    "backingstore",
                    "walk-prefetch-min-interval",
                    || Duration::from_millis(10),
                )?,
            };

            prefetch_manager(
                prefetch_config,
                repo.tree_resolver()?,
                filestore.clone(),
                parent_hint,
                walk_detector.clone(),
            )
        } else {
            // Stick a dummy channel in so we don't need to fuss with Option.
            flume::bounded(0).0
        };

        Ok(Inner {
            treestore,
            filestore,
            repo,
            mount_path: mount.to_path_buf(),
            extra_configs: extra_configs.to_vec(),
            create_time: Instant::now(),
            touch_file_mtime,
            already_reloading: AtomicBool::new(false),
            last_reload: Instant::now(),
            reload_check_interval: config
                .get_opt("backingstore", "reload-check-interval-secs")?
                .unwrap_or(Duration::from_secs(5)),
            reload_interval: config
                .get_opt("backingstore", "reload-interval-secs")?
                .unwrap_or(Duration::from_secs(300)),
            prefetch_send,
            walk_mode,
            walk_detector,
        })
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_blob(&self, fctx: FetchContext, node: &[u8]) -> Result<Option<Blob>> {
        self.maybe_reload().filestore.single(fctx, node)
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array.
    #[instrument(level = "trace", skip(self, resolve))]
    pub fn get_blob_batch<F>(&self, fctx: FetchContext, keys: Vec<Key>, resolve: F)
    where
        F: Fn(usize, Result<Option<Blob>>),
    {
        self.maybe_reload()
            .filestore
            .batch_with_callback(fctx, keys, resolve)
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_manifest(&self, node: &[u8]) -> Result<[u8; 20]> {
        let inner = self.maybe_reload();
        let hgid = HgId::from_slice(node)?;
        let root_tree_id = match inner.repo.tree_resolver()?.get_root_id(&hgid) {
            Ok(root_tree_id) => root_tree_id,
            Err(_e) => {
                // This call may fail with a `NotFoundError` if the revision in question
                // was added to the repository after we originally opened it. Invalidate
                // the repository and try again, in case our cached repo data is just stale.
                inner.repo.invalidate_all()?;
                inner.repo.tree_resolver()?.get_root_id(&hgid)?
            }
        };
        Ok(root_tree_id.into_byte_array())
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_tree(&self, fctx: FetchContext, node: &[u8]) -> Result<Option<Box<dyn TreeEntry>>> {
        self.maybe_reload().treestore.single(fctx, node)
    }

    /// Fetch tree contents in batch. Whenever a tree is fetched, the supplied `resolve` function is
    /// called with the tree content or an error message, and the index of the tree in the request
    /// array.
    #[instrument(level = "trace", skip(self, resolve))]
    pub fn get_tree_batch<F>(&self, fctx: FetchContext, keys: Vec<Key>, resolve: F)
    where
        F: Fn(usize, Result<Option<Box<dyn TreeEntry>>>),
    {
        self.maybe_reload()
            .treestore
            .batch_with_callback(fctx, keys, resolve)
    }

    pub fn get_file_aux(&self, fctx: FetchContext, node: &[u8]) -> Result<Option<FileAuxData>> {
        self.maybe_reload().filestore.single(fctx, node)
    }

    pub fn get_file_aux_batch<F>(&self, fctx: FetchContext, keys: Vec<Key>, resolve: F)
    where
        F: Fn(usize, Result<Option<FileAuxData>>),
    {
        self.maybe_reload()
            .filestore
            .batch_with_callback(fctx, keys, resolve)
    }

    pub fn get_tree_aux(&self, fctx: FetchContext, node: &[u8]) -> Result<Option<TreeAuxData>> {
        self.maybe_reload().treestore.single(fctx, node)
    }

    pub fn get_tree_aux_batch<F>(&self, fctx: FetchContext, keys: Vec<Key>, resolve: F)
    where
        F: Fn(usize, Result<Option<TreeAuxData>>),
    {
        self.maybe_reload()
            .treestore
            .batch_with_callback(fctx, keys, resolve)
    }

    pub fn dogfooding_host(&self) -> Result<bool> {
        Ok(RECENT_DOGFOODING_REQUESTS.get())
    }

    /// Forces backing store to rescan pack files or local indexes
    #[instrument(level = "trace", skip(self))]
    pub fn refresh(&self) {
        // We don't need maybe_reload() here. It doesn't make sense to
        // potentially reload everything right before refreshing it again
        // (although it wouldn't hurt).
        let inner = self.inner.load();

        inner.filestore.refresh().ok();
        inner.treestore.refresh().ok();
    }

    #[instrument(level = "trace", skip(self))]
    pub fn flush(&self) {
        // No need to maybe_reload() - flush intends to operate on current backingstore.
        // It wouldn't hurt, though, since reloading also flushes.
        self.inner.load().flush();
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_glob_files(
        &self,
        commit_id: &[u8],
        suffixes: Vec<String>,
        prefixes: Option<Vec<String>>,
    ) -> Result<Option<Vec<String>>> {
        // Lots of room for future optimizations here, such as handling the string conversion inside
        // the Response, probably by implementing map similar to how then is currently implemented.
        // Another option is to hand down the async object through to C++ when the FFI layer supports
        // it more robustly.
        let result = BlockingResponse::from_async(
            self.maybe_reload()
                .repo
                .eden_api()
                .map_err(|err| err.tag_network())?
                .suffix_query(CommitId::Hg(HgId::from_hex(commit_id)?), suffixes, prefixes),
        )?
        .entries
        .iter()
        .map(|res| res.file_path.to_string())
        .collect();
        Ok(Some(result))
    }

    #[instrument(level = "trace", skip(self))]
    pub fn witness_file_read(&self, path: &RepoPath, local: bool, pid: u32) {
        let inner = self.inner.load();

        if inner.walk_mode == WalkMode::Off {
            return;
        }

        let walk_changed = if local {
            inner.walk_detector.file_read(path, pid);
            false
        } else {
            inner.walk_detector.file_loaded(path, pid)
        };
        if !walk_changed {
            return;
        }

        inner.notify_prefetch();
    }

    #[instrument(level = "trace", skip(self))]
    pub fn witness_dir_read(
        &self,
        path: &RepoPath,
        local: bool,
        num_files: usize,
        num_dirs: usize,
        pid: u32,
    ) {
        let inner = self.inner.load();

        if inner.walk_mode == WalkMode::Off {
            return;
        }

        let walk_changed = if local {
            inner.walk_detector.dir_read(path, num_files, num_dirs, pid);
            false
        } else {
            inner
                .walk_detector
                .dir_loaded(path, num_files, num_dirs, pid)
        };
        if !walk_changed {
            return;
        }

        inner.notify_prefetch();
    }

    pub fn set_parent_hint(&self, parent_id: &str) {
        tracing::info!(parent_id, "setting parent hint");

        *self.parent_hint.write() = Some(parent_id.to_string());

        self.maybe_reload().notify_prefetch();
    }

    // Fully reload the stores if:
    //   - a touch file has a newer mtime than last time we checked, or
    //   - the touch file exists and didn't exist last time, or
    //   - sapling configs appear changed on disk
    //
    // The main purpose of reloading is to allow a running EdenFS to pick up
    // Sapling config changes that affect fetching/caching.
    //
    // We perform the check at most once every `reload_check_interval=5`
    // seconds. If we aren't reloading, we still swap out the Inner object
    // solely to reset the state we use to track the touch file (i.e. we keep
    // all the store objects the same). Any errors reloading are ignored and the
    // existing stores are used.
    //
    // We return an arc_swap::Guard so we only call inner.load() once normally.
    #[instrument(level = "trace", skip(self))]
    fn maybe_reload(&self) -> arc_swap::Guard<Arc<Inner>> {
        let inner = self.inner.load();

        if inner.create_time.elapsed() < inner.reload_check_interval {
            return inner;
        }

        tracing::debug!("checking if we need to reload");

        if inner.already_reloading.swap(true, Ordering::AcqRel) {
            tracing::debug!("another thread is already reloading");
            // No need to wait - just serve up the old one for now.
            return inner;
        }

        let since_last_reload = inner.last_reload.elapsed();

        let mut needs_reload = false;

        // If it has been at least `reload_interval`, check if sapling config has changed.
        if !inner.reload_interval.is_zero() && since_last_reload >= inner.reload_interval {
            if let Ok(info) = RepoMinimalInfo::from_repo_root(inner.repo.path().to_owned()) {
                if let Ok(config) =
                    configloader::hg::embedded_load(RepoInfo::Disk(&info), &inner.extra_configs)
                {
                    if let Some(reason) =
                        diff_config_files(&inner.repo.config().files(), &config.files())
                    {
                        tracing::info!("sapling config files differ: {reason}");
                        needs_reload = true;
                    } else {
                        tracing::debug!("sapling config files haven't changed");
                    }
                }
            }
        };

        let new_mtime = touch_file_mtime();

        tracing::debug!(last_reload=?since_last_reload, old_mtime=?inner.touch_file_mtime, ?new_mtime, "checking touch file");

        needs_reload |= new_mtime
            .as_ref()
            .is_some_and(|new_mtime| match &inner.touch_file_mtime {
                Some(old_mtime) => new_mtime > old_mtime,
                None => true,
            });

        let new_inner = if needs_reload {
            tracing::info!("reloading backing store");

            // We are actually going to reload. Flush first to make sure pending
            // cache writes are picked up by newly initialized backingstore.
            // There is no locking, so some cache writes could be missed by the reload.
            inner.flush();

            match Self::new_inner(
                inner.repo.path(),
                &inner.mount_path,
                &inner.extra_configs,
                new_mtime,
                self.parent_hint.clone(),
                inner.walk_detector.clone(),
            ) {
                Ok(mut new_inner) => {
                    new_inner.last_reload = Instant::now();
                    new_inner
                }
                Err(err) => {
                    tracing::warn!(?err, "error reloading backingstore");
                    inner.as_ref().soft_reload(new_mtime)
                }
            }
        } else {
            tracing::debug!("not reloading backing store");
            inner.as_ref().soft_reload(new_mtime)
        };

        self.inner.store(Arc::new(new_inner));

        if needs_reload {
            // Flush the old stores again right after the swaperoo. This should help
            // reduce the window for missed cache writes. This flush is effective even
            // though we have already created new stores since the scmstore indexedlogs
            // automatically notice things have changed on disk during the read path.
            inner.flush();
        }

        self.inner.load()
    }
}

fn diff_config_files(
    old: &[(PathBuf, Option<ContentHash>)],
    new: &[(PathBuf, Option<ContentHash>)],
) -> Option<String> {
    let mut new: HashMap<PathBuf, Option<ContentHash>> = new.iter().cloned().collect();

    for (old_path, old_hash) in old.iter() {
        if let Some(new_hash) = new.remove(old_path) {
            let mismatch = match (old_hash, new_hash) {
                (None, None) => false,
                (None, Some(_)) => true,
                (Some(_), None) => true,
                (Some(old_hash), Some(ref new_hash)) => old_hash != new_hash,
            };

            if mismatch {
                return Some(format!("file {} metadata mismatch", old_path.display()));
            }
        } else {
            return Some(format!("file {} was deleted", old_path.display()));
        }
    }

    // Anything left is a new file we didn't have last time.
    new.keys()
        .next()
        .map(|added| format!("file {} was added", added.display()))
}

impl Inner {
    // Perform a shallow clone, retaining stores but resetting state related to the touch file.
    fn soft_reload(&self, touch_file_mtime: Option<SystemTime>) -> Self {
        Self {
            filestore: self.filestore.clone(),
            treestore: self.treestore.clone(),
            repo: self.repo.clone(),
            mount_path: self.mount_path.clone(),
            extra_configs: self.extra_configs.clone(),

            touch_file_mtime,
            create_time: Instant::now(),
            last_reload: self.last_reload,
            already_reloading: AtomicBool::new(false),
            reload_check_interval: self.reload_check_interval,
            reload_interval: self.reload_interval,

            prefetch_send: self.prefetch_send.clone(),
            walk_mode: self.walk_mode,
            walk_detector: self.walk_detector.clone(),
        }
    }

    fn flush(&self) {
        self.filestore.flush().ok();
        self.treestore.flush().ok();
    }

    fn notify_prefetch(&self) {
        if self.walk_mode != WalkMode::Prefetch {
            return;
        }

        let _ = self.prefetch_send.try_send(());
    }
}

fn touch_file_mtime() -> Option<SystemTime> {
    let path = if cfg!(windows) {
        std::env::var_os("PROGRAMDATA")
            .map(|dir| PathBuf::from(dir).join(r"Facebook\Mercurial\eden_reload"))
    } else {
        Some(PathBuf::from("/etc/mercurial/eden_reload"))
    };

    let path = path?;
    let res = path.metadata();

    tracing::debug!(?path, ?res, "statting touch file");

    res.ok()?.modified().ok()
}

/// Given a single point local fetch function, and a "streaming" (via iterator)
/// remote fetch function, provide `batch_with_callback` for ease-of-use.
trait LocalRemoteImpl<IntermediateType, OutputType = IntermediateType>
where
    IntermediateType: Into<OutputType>,
{
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<IntermediateType>>;
    fn get_single(&self, fctx: FetchContext, path: &RepoPath, id: HgId)
    -> Result<IntermediateType>;
    fn get_batch_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, IntermediateType)>>>;

    // The following methods are "derived" from the above.

    fn single(&self, fctx: FetchContext, node: &[u8]) -> Result<Option<OutputType>> {
        let hgid = HgId::from_slice(node)?;
        if fctx.mode().is_local() {
            let maybe_value = self
                .get_local_single(RepoPath::empty(), hgid)?
                .map(|v| v.into());
            Ok(maybe_value)
        } else {
            // FetchMode::RemoteOnly and FetchMode::AllowRemote
            let value = self.get_single(fctx, RepoPath::empty(), hgid)?;
            let value = value.into();
            Ok(Some(value))
        }
    }

    fn batch_with_callback<F>(&self, fctx: FetchContext, keys: Vec<Key>, resolve: F)
    where
        F: Fn(usize, Result<Option<OutputType>>),
    {
        if fctx.mode().is_local() {
            // PERF: In some cases this might be sped up using threads in theory.
            // But this needs to be backed by real benchmark data. Besides, edenfs
            // does not call into this path often.
            for (i, key) in keys.iter().enumerate() {
                let result = self.get_local_single(&key.path, key.hgid);
                let result: Result<Option<OutputType>> = match result {
                    Ok(Some(v)) => Ok(Some(v.into())),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                };
                resolve(i, result);
            }
        } else {
            let ignore_result = fctx.mode().ignore_result();

            let mut key_to_index = indexed_keys(&keys);
            let mut remaining = keys.len();
            let mut errors = Vec::new();
            match self.get_batch_iter(fctx, keys) {
                Err(e) => errors.push(e),
                Ok(iter) => {
                    for entry in iter {
                        let (key, data) = match entry {
                            Err(e) => {
                                errors.push(e);
                                continue;
                            }
                            Ok(v) => v,
                        };
                        if let Some(entry) = key_to_index.get_mut(&key) {
                            if let Some(index) = *entry {
                                *entry = None;
                                remaining = remaining.saturating_sub(1);
                                let result = Ok(Some(data.into()));
                                resolve(index, result);
                            }
                        }
                    }
                }
            }
            if remaining > 0 {
                if ignore_result {
                    // In ignore_result mode, we (intentionally) don't get any results. Propagate as `None`.
                    for (_key, entry) in key_to_index.into_iter() {
                        if let Some(index) = entry {
                            resolve(index, Ok(None));
                        }
                    }
                } else {
                    // In ffi.rs, the error is converted to a String where, later, empty string is assumed to mean no error.
                    // Ensure we have some error.
                    if errors.is_empty() {
                        errors.push(anyhow!("{remaining} items in batch missing, but got no errors from get_batch_iter"));
                    }

                    // Report errors. We don't know the index -> error mapping so
                    // we bundle all errors we received.
                    let error = ErrorCollection(Arc::new(errors));
                    for (_key, entry) in key_to_index.into_iter() {
                        if let Some(index) = entry {
                            resolve(index, Err(error.clone().into()));
                        }
                    }
                }
            }
        }
    }
}

/// Read file content.
impl LocalRemoteImpl<Blob> for Arc<dyn FileStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<Blob>> {
        self.get_local_content(path, id)
    }
    fn get_single(&self, fctx: FetchContext, path: &RepoPath, id: HgId) -> Result<Blob> {
        self.get_content(fctx, path, id)
    }
    fn get_batch_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Blob)>>> {
        Ok(Box::new(self.get_content_iter(fctx, keys)?))
    }
}

/// Read file aux.
impl LocalRemoteImpl<FileAuxData> for Arc<dyn FileStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<FileAuxData>> {
        self.get_local_aux(path, id)
    }
    fn get_single(&self, fctx: FetchContext, path: &RepoPath, id: HgId) -> Result<FileAuxData> {
        self.get_aux(fctx, path, id)
    }
    fn get_batch_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, FileAuxData)>>> {
        self.get_aux_iter(fctx, keys)
    }
}

/// Read tree content.
impl LocalRemoteImpl<Box<dyn TreeEntry>> for Arc<dyn TreeStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<Box<dyn TreeEntry>>> {
        self.get_local_tree(path, id)
    }
    fn get_single(
        &self,
        fctx: FetchContext,
        path: &RepoPath,
        id: HgId,
    ) -> Result<Box<dyn TreeEntry>> {
        match self
            .get_tree_iter(fctx, vec![Key::new(path.to_owned(), id)])?
            .next()
        {
            Some(Ok((_key, tree))) => Ok(tree),
            Some(Err(e)) => Err(e),
            None => Err(anyhow::format_err!("{}@{}: not found remotely", path, id)),
        }
    }
    fn get_batch_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Box<dyn TreeEntry>)>>> {
        self.get_tree_iter(fctx, keys)
    }
}

/// Read tree aux.
impl LocalRemoteImpl<TreeAuxData> for Arc<dyn TreeStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<TreeAuxData>> {
        self.get_local_tree_aux_data(path, id)
    }
    fn get_single(&self, fctx: FetchContext, path: &RepoPath, id: HgId) -> Result<TreeAuxData> {
        self.get_tree_aux_data(fctx, path, id)
    }
    fn get_batch_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, TreeAuxData)>>> {
        self.get_tree_aux_data_iter(fctx, keys)
    }
}

/// This type is just for display.
#[derive(Debug, Clone)]
struct ErrorCollection(Arc<Vec<anyhow::Error>>);

impl fmt::Display for ErrorCollection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some((first, rest)) = self.0.split_first() {
            first.fmt(f)?;

            let n = rest.len();
            if n > 0 {
                write!(f, "\n-- and {n} more errors --\n")?;
                for e in rest {
                    e.fmt(f)?;
                    write!(f, "\n--\n")?;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for ErrorCollection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.first().map(|e| e.as_ref())
    }
}

/// Index &[Key] so they can be converted back to the index.
fn indexed_keys(keys: &[Key]) -> HashMap<Key, Option<usize>> {
    keys.iter()
        .cloned()
        .enumerate()
        .map(|(i, k)| (k, Some(i)))
        .collect()
}

impl Drop for BackingStore {
    fn drop(&mut self) {
        // Make sure that all the data that was fetched is written to the hgcache.
        self.flush();
    }
}
