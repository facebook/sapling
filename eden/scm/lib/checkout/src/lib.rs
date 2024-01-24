/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use atexit::AtExit;
use crossbeam::channel;
#[cfg(windows)]
use fs_err as fs;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use minibytes::Bytes;
use parking_lot::Mutex;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use pathmatcher::UnionMatcher;
use progress_model::ProgressBar;
use progress_model::Registry;
use repo::repo::Repo;
use storemodel::FileStore;
use termlogger::TermLogger;
use tracing::debug;
use tracing::instrument;
use tracing::warn;
use treestate::dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::hgid::MF_ADDED_NODE_ID;
use types::hgid::MF_MODIFIED_NODE_ID;
use types::hgid::NULL_ID;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::UpdateFlag;
use vfs::VFS;
use workingcopy::sparse;
use workingcopy::workingcopy::LockedWorkingCopy;

#[allow(dead_code)]
mod actions;
pub mod clone;
#[allow(dead_code)]
mod conflict;
#[cfg(feature = "eden")]
pub mod edenfs;
pub mod errors;
#[allow(dead_code)]
mod merge;

pub use actions::Action;
pub use actions::ActionMap;
use configmodel::Config;
use configmodel::ConfigExt;
pub use conflict::Conflict;
pub use merge::Merge;
pub use merge::MergeResult;
use status::FileStatus;
use status::Status;

// Affects progress update frequency and thread count for small checkout.
const VFS_BATCH_SIZE: usize = 128;

#[derive(PartialEq)]
pub enum CheckoutMode {
    Force,
    NoConflict,
    Merge,
}

/// Contains lists of files to be removed / updated during checkout.
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files that needs their content updated.
    update_content: HashMap<RepoPathBuf, UpdateContentAction>,
    filtered_update_content: HashMap<RepoPathBuf, UpdateContentAction>,
    /// Files that only need X flag updated.
    update_meta: Vec<UpdateMetaAction>,
    progress: Option<Arc<Mutex<CheckoutProgress>>>,
    checkout: Checkout,
}

struct CheckoutProgress {
    file: File,
    vfs: VFS,
    /// Recording of the file time and size that have already been written.
    state: HashMap<RepoPathBuf, (HgId, u128, u64)>,
}

/// Update content and (possibly) metadata on the file
#[derive(Clone, Debug)]
struct UpdateContentAction {
    /// If content has changed, HgId of new content.
    content_hgid: HgId,
    /// New file type.
    file_type: FileType,
}

/// Only update metadata on the file, do not update content
#[derive(Debug)]
struct UpdateMetaAction {
    /// Path to file.
    path: RepoPathBuf,
    /// true if need to set executable flag, false if need to remove it.
    set_x_flag: bool,
}

/// Errors during checkout.
#[derive(Default, Debug)]
pub struct CheckoutStats {
    /// Error on doing `Work`.
    pub remove_failed: Vec<(RepoPathBuf, Error)>,
    set_exec_failed: Vec<(RepoPathBuf, Error)>,
    write_failed: Vec<(RepoPathBuf, Error)>,

    /// Errors not associated with a path.
    other_failed: Vec<Error>,
}

enum Work {
    Write(Key, Bytes, UpdateFlag),
    SetExec(RepoPathBuf, bool),
    Remove(RepoPathBuf),
}

const DEFAULT_CONCURRENCY: usize = 16;
const MAX_CHECK_UNKNOWN: usize = 5000;

#[derive(Clone)]
pub struct Checkout {
    vfs: VFS,
    concurrency: usize,
}

impl Checkout {
    pub fn default_config(vfs: VFS) -> Self {
        Self {
            vfs,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    pub fn from_config(vfs: VFS, config: &dyn Config) -> Result<Self> {
        let concurrency = config
            .get_opt("nativecheckout", "concurrency")
            .map_err(|e| format_err!("Failed to parse nativecheckout.concurrency: {}", e))?;
        let concurrency = concurrency.unwrap_or(DEFAULT_CONCURRENCY);
        Ok(Self { vfs, concurrency })
    }

    pub fn plan_action_map(&self, map: ActionMap) -> CheckoutPlan {
        CheckoutPlan::from_action_map(self.clone(), map)
    }
}

impl CheckoutPlan {
    fn from_action_map(checkout: Checkout, map: ActionMap) -> Self {
        let mut remove = vec![];
        let mut update_content = HashMap::new();
        let mut update_meta = vec![];
        for (path, action) in map.into_iter() {
            match action {
                Action::Remove => remove.push(path),
                Action::UpdateExec(set_x_flag) => {
                    update_meta.push(UpdateMetaAction { path, set_x_flag })
                }
                Action::Update(up) => {
                    update_content.insert(path, UpdateContentAction::new(up.to));
                }
            }
        }
        let filtered_update_content = update_content.clone();
        Self {
            remove,
            update_content,
            filtered_update_content,
            update_meta,
            progress: None,
            checkout,
        }
    }

    pub fn add_progress(&mut self, path: &Path) -> Result<()> {
        let vfs = &self.checkout.vfs;
        let progress = if path.exists() {
            debug!(?path, "loading progress");
            match CheckoutProgress::load(path, vfs.clone()) {
                Ok(p) => p,
                Err(err) => {
                    warn!(?err, "failed loading progress");
                    CheckoutProgress::new(path, vfs.clone())?
                }
            }
        } else {
            CheckoutProgress::new(path, vfs.clone())?
        };
        self.filtered_update_content = progress.filter_already_written(&self.update_content);
        self.progress = Some(Arc::new(Mutex::new(progress)));
        Ok(())
    }

    /// Applies plan to the root using store to fetch data.
    ///
    /// Fails fast on critical errors like unable to create threads.
    /// Otherwise, try to keep going as much as possible.
    ///
    /// Returning `Ok` when there is no fatal errors.
    /// Not able to remove files are considered not fatal.
    /// The returned error could also be `CheckoutStats` when there are fatal errors.
    #[instrument(skip_all, err)]
    pub fn apply_store(&self, store: &dyn FileStore) -> Result<CheckoutStats> {
        let vfs = &self.checkout.vfs;

        let skipped_count = self.update_content.len() - self.filtered_update_content.len();
        debug!(skipped_count, "skipped files based on progress");

        let total = self.filtered_update_content.len() + self.remove.len() + self.update_meta.len();
        let bar = &ProgressBar::new("Updating", total as u64, "files");
        Registry::main().register_progress_bar(bar);

        // Checkout result.
        let mut stats = CheckoutStats::default();

        // Task to write file contents using threads.
        let actions: HashMap<_, _> = self
            .filtered_update_content
            .iter()
            .map(|(p, u)| (Key::new(p.clone(), u.content_hgid.clone()), u.clone()))
            .collect();
        let keys: Vec<_> = actions.keys().cloned().collect();
        let fetch_data_iter = store.get_content_iter(keys)?;

        let stats = thread::scope(|s| -> Result<CheckoutStats> {
            let (tx, rx) = channel::unbounded::<Work>();
            let (err_tx, err_rx) = channel::unbounded();
            let (progress_tx, progress_rx) = channel::unbounded();

            // On Ctrl+C or error, write the "progress" file to help resume.
            let progress = self.progress.clone();
            let support_resume = progress.is_some();
            tracing::debug!(support_resume = support_resume, "apply_store");

            let on_abort = AtExit::new(Box::new(move || {
                if let Some(progress) = progress {
                    tracing::debug!("writing progress (on abort)");
                    let id_paths: Vec<_> = progress_rx.into_iter().collect();
                    progress.lock().record_writes(&id_paths);
                }
            }));

            // Spawn writer threads. Thread count is 1 for simple changes.
            let n = self.vfs_worker_count(total);
            assert!(n >= 1);
            for i in 1..=n {
                let vfs = vfs.clone();
                let rx = rx.clone();
                let err_tx = err_tx.clone();
                let progress_tx = progress_tx.clone();
                let b = thread::Builder::new().name(format!("checkout-{}/{}", i, n));
                b.spawn_scoped(s, move || {
                    let mut bar_count = 0;
                    while let Ok(work) = rx.recv() {
                        let result = match &work {
                            Work::Write(key, data, flag) => {
                                vfs.write(&key.path, data, *flag).map(|_| ())
                            }
                            Work::SetExec(path, exec) => {
                                vfs.set_executable(path, *exec).map(|_| ())
                            }
                            Work::Remove(path) => vfs.remove(path),
                        };
                        bar_count += 1;
                        if bar_count >= VFS_BATCH_SIZE {
                            bar.increase_position(bar_count as _);
                            bar.set_message(work.path().to_string());
                            bar_count = 0;
                        }
                        if let Err(e) = result {
                            if err_tx.send((Some(work), e)).is_err() {
                                break;
                            }
                            // Keep going.
                            continue;
                        }
                        if support_resume {
                            if let Work::Write(key, ..) = work {
                                let _ = progress_tx.send((key.hgid, key.path));
                            }
                        }
                    }
                    bar.increase_position(bar_count as _);
                })?;
            }

            drop(rx);
            drop(progress_tx);

            // Read loop for writer threads.
            for path in &self.remove {
                tx.send(Work::Remove(path.to_owned()))?;
            }
            for action in &self.update_meta {
                tx.send(Work::SetExec(action.path.to_owned(), action.set_x_flag))?;
            }
            for entry in fetch_data_iter {
                let (key, data) = match entry {
                    Err(e) => {
                        let _ = err_tx.send((None, e));
                        // Keep going.
                        continue;
                    }
                    Ok(v) => v,
                };
                let action = actions
                    .get(&key)
                    .ok_or_else(|| format_err!("Storage returned unknown key {}", key))?;
                let flag = type_to_flag(&action.file_type);
                tx.send(Work::Write(key, data, flag))?;
            }
            drop(tx);

            // Error.
            if fail::eval("checkout-post-progress", |_| ()).is_some() {
                err_tx.send((
                    None,
                    anyhow::format_err!("Error set by checkout-post-progress FAILPOINTS"),
                ))?;
            }
            drop(err_tx);

            // Turn errors into CheckoutStats.
            while let Ok((maybe_work, err)) = err_rx.recv() {
                match maybe_work {
                    None => stats.other_failed.push(err),
                    Some(Work::Remove(path)) => stats.remove_failed.push((path, err)),
                    Some(Work::SetExec(path, _)) => stats.set_exec_failed.push((path, err)),
                    Some(Work::Write(key, _, _)) => stats.write_failed.push((key.path, err)),
                }
            }

            let is_fatal = stats.is_fatal();
            tracing::debug!(is_fatal = is_fatal, "apply_store");
            if is_fatal {
                return Err(stats.into());
            } else {
                // No need to write progress file on success.
                on_abort.cancel();
            }

            Ok(stats)
        });

        // Windows symlink fixes. This should happen after vfs writes.
        // The symlink fixes might generate new errors.
        #[cfg(windows)]
        let mut stats = stats;
        #[cfg(windows)]
        {
            if vfs.supports_symlinks() {
                let symlinks = self
                    .filtered_update_content
                    .iter()
                    .filter_map(|(p, a)| {
                        if a.file_type == FileType::Symlink {
                            Some(p.as_ref())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if let Err(e) = update_symlinks(&symlinks, vfs) {
                    if let Ok(stats) = stats.as_mut() {
                        stats.other_failed.push(e);
                    }
                }
            }
        }

        stats
    }

    fn vfs_worker_count(&self, total: usize) -> usize {
        match thread::available_parallelism() {
            Ok(v) => v
                .get()
                .min(self.checkout.concurrency)
                .min(total / VFS_BATCH_SIZE),
            Err(_) => 1,
        }
        .max(1)
    }

    pub fn apply_store_dry_run(&self, store: &dyn FileStore) -> Result<(usize, u64)> {
        let keys = self
            .filtered_update_content
            .iter()
            .map(|(p, u)| Key::new(p.clone(), u.content_hgid.clone()));
        let keys: Vec<_> = keys.collect();
        let (mut count, mut size) = (0, 0);
        let iter = store.get_content_iter(keys)?;
        for result in iter {
            let (_key, data) = result?;
            count += 1;
            size += data.len() as u64;
        }
        Ok((count, size))
    }

    pub fn check_conflicts(&self, status: &Status) -> Vec<&RepoPath> {
        let mut conflicts = vec![];
        for file in self.all_files() {
            // Unknown files are handled separately in check_unknown_files
            if !matches!(status.status(file), None | Some(FileStatus::Unknown)) {
                conflicts.push(file.as_repo_path());
            }
        }
        conflicts
    }

    pub fn check_unknown_files(
        &self,
        manifest: &impl Manifest,
        store: &dyn FileStore,
        tree_state: &mut TreeState,
        status: &Status,
    ) -> Result<Vec<RepoPathBuf>> {
        let vfs = &self.checkout.vfs;
        let mut check_content = vec![];

        let unknown: Vec<&RepoPathBuf> = status.unknown().collect();

        let bar = ProgressBar::new_adhoc("Checking untracked", unknown.len() as u64, "files");

        for file in unknown {
            bar.increase_position(1);
            bar.set_message(file.to_string());

            if !self.filtered_update_content.contains_key(file) {
                continue;
            }

            let state = if vfs.case_sensitive() {
                tree_state.get(file)?
            } else {
                let matches = tree_state.get_keys_ignorecase(file)?;
                let mut matches = matches.into_iter();
                let next = matches.next();
                match next {
                    None => None,
                    Some(next) => {
                        if let Some(extra) = matches.next() {
                            warn!(
                                "TreeState::get_ignorecase found multiple files on case insensitive fs for {}: {:?}, {:?}",
                                file, next, extra
                            );
                        }
                        tree_state.get(next)?
                    }
                }
            };
            let unknown = match state {
                None => true,
                Some(state) => !state.state.intersects(
                    StateFlags::EXIST_P1 | StateFlags::EXIST_P2 | StateFlags::EXIST_NEXT,
                ),
            };
            if unknown && matches!(vfs.is_file(file), Ok(true)) {
                let repo_path = file.as_repo_path();
                let hgid = match manifest.get_file(repo_path)? {
                    Some(m) => m.hgid,
                    None => bail!(
                        "{} not found in manifest when checking for unknown files",
                        repo_path
                    ),
                };
                let key = Key::new(file.clone(), hgid);
                check_content.push(key);
            }
        }

        if check_content.len() > MAX_CHECK_UNKNOWN {
            warn!(
                "Working directory has {} untracked files, not going to check their content. Use --clean to overwrite files without checking",
                check_content.len()
            );
            let unknowns = check_content.into_iter().map(|k| k.path).collect();
            return Ok(unknowns);
        }

        let mut paths = Vec::new();
        for entry in store.get_content_iter(check_content)? {
            let (key, data) = entry?;
            if let Some(path) = Self::check_content(vfs, key, data) {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn check_content(vfs: &VFS, key: Key, data: Bytes) -> Option<RepoPathBuf> {
        let path = &key.path;
        match Self::check_file(vfs, data, path) {
            Err(err) => {
                warn!("Can not check {}: {}", path, err);
                Some(key.path)
            }
            Ok(false) => Some(key.path),
            Ok(true) => None,
        }
    }

    fn check_file(vfs: &VFS, expected_content: Bytes, path: &RepoPath) -> Result<bool> {
        let actual_content = vfs.read(path)?;
        Ok(actual_content.eq(&expected_content))
    }

    pub fn removed_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.remove.iter()
    }

    pub fn updated_content_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_content.keys()
    }

    pub fn updated_meta_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_meta.iter().map(|u| &u.path)
    }

    pub fn all_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_content
            .keys()
            .chain(self.remove.iter())
            .chain(self.update_meta.iter().map(|u| &u.path))
    }

    /// Returns (updated, removed)
    pub fn stats(&self) -> (usize, usize) {
        (
            self.update_meta.len() + self.update_content.len(),
            self.remove.len(),
        )
    }

    pub fn vfs(&self) -> &VFS {
        &self.checkout.vfs
    }

    #[cfg(test)]
    pub fn empty(vfs: VFS) -> Self {
        Self {
            remove: vec![],
            update_content: HashMap::new(),
            filtered_update_content: HashMap::new(),
            update_meta: vec![],
            progress: None,
            checkout: Checkout::default_config(vfs),
        }
    }
}

impl CheckoutStats {
    fn is_fatal(&self) -> bool {
        !self.write_failed.is_empty()
            || !self.other_failed.is_empty()
            || !self.set_exec_failed.is_empty()
    }
}

impl fmt::Display for CheckoutStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (_path, err) in &self.write_failed {
            err.fmt(f)?;
        }
        for (_path, err) in &self.set_exec_failed {
            err.fmt(f)?;
        }
        for (_path, err) in &self.remove_failed {
            err.fmt(f)?;
        }
        for err in &self.other_failed {
            write!(f, "checkout error: {}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for CheckoutStats {
    // Consider impl sources() after
    // https://github.com/rust-lang/rust/issues/58520
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some((_path, err)) = self.write_failed.first() {
            return Some(err.root_cause());
        }
        None
    }
}

impl CheckoutProgress {
    pub fn new(path: &Path, vfs: VFS) -> Result<Self> {
        Ok(CheckoutProgress {
            file: util::file::create(path)?,
            vfs,
            state: HashMap::new(),
        })
    }

    /// Loads the serialized checkout progress from disk. The format is one row per file written,
    /// consisting of space separated hg file hash, mtime in milliseconds, file length, and file
    /// path and a trailing \0 character.
    ///
    ///   <40_char_hg_hash> <mtime_in_millis> <written_file_length> <file_path>\0
    pub fn load(path: &Path, vfs: VFS) -> Result<Self> {
        let mut state: HashMap<RepoPathBuf, (HgId, u128, u64)> = HashMap::new();

        let file = util::file::open(path, "r")?;
        let mut reader = BufReader::new(file);
        let mut buffer = vec![];
        loop {
            reader.read_until(0, &mut buffer)?;
            if buffer.is_empty() {
                break;
            }
            let (path, (hgid, time, size)) = match (|| -> Result<_> {
                let mut split = buffer.splitn(4, |c| *c == b' ');
                let hgid = HgId::from_hex(
                    split
                        .next()
                        .ok_or_else(|| anyhow!("invalid checkout update hgid format"))?,
                )?;

                let time = std::str::from_utf8(
                    split
                        .next()
                        .ok_or_else(|| anyhow!("invalid checkout update time format"))?,
                )?
                .parse::<u128>()?;

                let size = std::str::from_utf8(
                    split
                        .next()
                        .ok_or_else(|| anyhow!("invalid checkout update size format"))?,
                )?
                .parse::<u64>()?;

                let path = split
                    .next()
                    .ok_or_else(|| anyhow!("invalid checkout update path format"))?;
                let path = &path[..path.len() - 1];
                let path = RepoPathBuf::from_string(std::str::from_utf8(path)?.to_string())?;

                Ok((path, (hgid, time, size)))
            })() {
                Ok(entry) => entry,
                Err(_) => {
                    buffer.clear();
                    continue;
                }
            };

            state.insert(path, (hgid, time, size));
            buffer.clear();
        }

        Ok(CheckoutProgress {
            file: util::file::open(path, "ca")?,
            vfs,
            state,
        })
    }

    // PERF: vfs.metadata should not require mut self.
    fn record_writes(&mut self, paths: &[(HgId, RepoPathBuf)]) {
        for (hgid, path) in paths {
            // Don't report write failures, just let the checkout continue.
            let _ = (|| -> Result<()> {
                let stat = self.vfs.metadata(path)?;
                let time = stat
                    .modified()?
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_millis();

                self.file
                    .write_all(
                        format!("{} {} {} {}\0", hgid.to_hex(), time, stat.len(), path).as_bytes(),
                    )
                    .map_err(|e| e.into())
            })();
        }
    }

    fn filter_already_written<'a>(
        &self,
        actions: &HashMap<RepoPathBuf, UpdateContentAction>,
    ) -> HashMap<RepoPathBuf, UpdateContentAction> {
        // TODO: This should be done in parallel. Maybe with the new vfs async batch APIs?
        let bar = ProgressBar::new_adhoc("Filtering existing", actions.len() as u64, "files");
        actions
            .iter()
            .filter(move |(path, action)| {
                if let Some((hgid, time, size)) = &self.state.get(*path) {
                    if *hgid != action.content_hgid {
                        return true;
                    }

                    bar.increase_position(1);
                    bar.set_message(path.to_string());

                    if let Ok(stat) = self.vfs.metadata(path) {
                        let time_matches = stat
                            .modified()
                            .map(|t| {
                                t.duration_since(SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_millis() == *time)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);
                        if time_matches && &stat.len() == size {
                            return false;
                        }
                    }
                }
                true
            })
            .map(|(p, u)| (p.clone(), u.clone()))
            .collect()
    }
}

fn type_to_flag(ft: &FileType) -> UpdateFlag {
    match ft {
        FileType::Regular => UpdateFlag::Regular,
        FileType::Executable => UpdateFlag::Executable,
        FileType::Symlink => UpdateFlag::Symlink,
        FileType::GitSubmodule => {
            panic!("bug: GitSubmodule should be filtered out earlier by ActionMap")
        }
    }
}

impl UpdateContentAction {
    pub fn new(meta: FileMetadata) -> Self {
        Self {
            content_hgid: meta.hgid,
            file_type: meta.file_type,
        }
    }
}

impl Work {
    fn path(&self) -> &RepoPath {
        match self {
            Self::Write(key, ..) => &key.path,
            Self::SetExec(path, ..) => path,
            Self::Remove(path) => path,
        }
    }
}

impl AsRef<RepoPath> for UpdateMetaAction {
    fn as_ref(&self) -> &RepoPath {
        &self.path
    }
}

impl fmt::Display for CheckoutPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.remove {
            writeln!(f, "rm {}", r)?;
        }
        for (p, u) in &self.update_content {
            let ft = match u.file_type {
                FileType::Executable => "(x)",
                FileType::Symlink => "(s)",
                FileType::Regular => "",
                FileType::GitSubmodule => continue,
            };
            writeln!(f, "up {}=>{}{}", p, u.content_hgid, ft)?;
        }
        for u in &self.update_meta {
            let ch = if u.set_x_flag { "+x" } else { "-x" };
            writeln!(f, "{} {}", ch, u.path)?;
        }
        Ok(())
    }
}

pub fn file_state(vfs: &VFS, path: &RepoPath) -> Result<FileStateV2> {
    let meta = vfs.metadata(path)?;
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    #[cfg(windows)]
    let mode = if meta.is_symlink() { 0o120644 } else { 0o644 };
    let mtime = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let mtime = truncate_u64("mtime", path, mtime);
    let size = meta.len();
    let size = truncate_u64("size", path, size);
    let state = StateFlags::EXIST_P1 | StateFlags::EXIST_NEXT;
    Ok(FileStateV2 {
        mode,
        size,
        mtime,
        state,
        copied: None,
    })
}

fn truncate_u64(f: &str, path: &RepoPath, v: u64) -> i32 {
    const RANGE_MASK: u64 = 0x7FFFFFFF;
    let truncated = v & RANGE_MASK;
    if truncated != v {
        warn!("{} for {} is truncated {}=>{}", f, path, v, truncated);
    }
    truncated as i32
}

pub fn checkout(
    lgr: &TermLogger,
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    mut maybe_bookmark: Option<String>,
    update_mode: CheckoutMode,
) -> Result<Option<(usize, usize)>> {
    let stats = if repo.requirements.contains("eden") {
        #[cfg(feature = "eden")]
        {
            edenfs::edenfs_checkout(lgr, repo, wc, target_commit, update_mode)?;
            None
        }

        #[cfg(not(feature = "eden"))]
        bail!("checkout() called on eden working copy on non-eden build");
    } else {
        Some(filesystem_checkout(
            lgr,
            repo,
            wc,
            target_commit,
            update_mode,
        )?)
    };

    let local_bms = repo.local_bookmarks()?;
    if !maybe_bookmark
        .as_ref()
        .is_some_and(|bm| local_bms.contains_key(bm))
    {
        maybe_bookmark = None;
    }

    let current_bookmark = wc.active_bookmark()?;
    if maybe_bookmark != current_bookmark {
        match (&current_bookmark, &maybe_bookmark) {
            // TODO: color bookmark name
            (Some(old), Some(new)) => {
                lgr.info(format!("(changing active bookmark from {old} to {new})"))
            }
            (None, Some(new)) => lgr.info(format!("(activating bookmark {new})")),
            (Some(old), None) => lgr.info(format!("(leaving bookmark {old})")),
            (None, None) => {}
        }

        wc.set_active_bookmark(maybe_bookmark)?;
    }

    Ok(stats)
}

fn file_type(vfs: &VFS, path: &RepoPath) -> FileType {
    match vfs.metadata(path) {
        Err(err) => {
            tracing::warn!(?err, %path, "error statting modified file");
            FileType::Regular
        }
        Ok(md) => {
            let md: workingcopy::metadata::Metadata = md.into();
            if md.is_symlink(vfs) {
                FileType::Symlink
            } else if md.is_executable(vfs) {
                FileType::Executable
            } else {
                FileType::Regular
            }
        }
    }
}

pub fn filesystem_checkout(
    lgr: &TermLogger,
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    update_mode: CheckoutMode,
) -> Result<(usize, usize)> {
    let current_commit = wc.parents()?.into_iter().next().unwrap_or(NULL_ID);

    let tree_resolver = repo.tree_resolver()?;
    let mut current_mf = tree_resolver.get(&current_commit)?;
    let target_mf = tree_resolver.get(&target_commit)?;

    let (sparse_matcher, sparse_change) =
        create_sparse_matchers(repo, wc.vfs(), &current_mf, &target_mf)?;

    // Overlay manifest with "status" info to include outstanding working copy changes.
    let status = wc.status(sparse_matcher.clone(), false, repo.config(), lgr)?;

    if update_mode == CheckoutMode::Force {
        // With --clean, mix on our working copy changes so they are "undone" by
        // the diff w/ target manifest.
        overlay_working_changes(wc.vfs(), &mut current_mf, &status)?;

        // --clean clears out any merge state
        wc.clear_merge_state()?;
    }

    let progress_path: Option<PathBuf> = if repo.config().get_or_default("checkout", "resumable")? {
        Some(wc.dot_hg_path().join("updateprogress"))
    } else {
        None
    };

    // 1. Create the plan
    let plan = create_plan(
        wc.vfs(),
        repo.config(),
        &current_mf,
        &target_mf,
        &sparse_matcher,
        sparse_change,
        progress_path,
    )?;

    if update_mode != CheckoutMode::Force {
        // 2. Check if status is dirty
        check_conflicts(lgr, repo, wc, &plan, &target_mf, &status)?;
    }

    // 3. Signal that an update is being performed

    let updatestate_path = wc.dot_hg_path().join("updatestate");

    util::file::atomic_write(&updatestate_path, |f| {
        write!(f, "{}", target_commit.to_hex())
    })?;

    // 4. Execute the plan
    let apply_result = plan.apply_store(repo.file_store()?.as_ref())?;

    for (path, err) in apply_result.remove_failed {
        lgr.warn(format!("update failed to remove {}: {:#}!\n", path, err));
    }

    // 5. Update the treestate parents, dirstate
    wc.set_parents(vec![target_commit], None)?;
    record_updates(&plan, wc.vfs(), &mut wc.treestate().lock())?;
    dirstate::flush(
        wc.vfs().root(),
        &mut wc.treestate().lock(),
        repo.locker(),
        None,
        None,
    )?;

    util::file::unlink_if_exists(&updatestate_path)?;

    Ok(plan.stats())
}

// Apply outstanding working copy changes to the given manifest. This includes
// the working copy changes in the diff between the working copy manifest and
// the checkout target manifest.
fn overlay_working_changes(vfs: &VFS, mf: &mut TreeManifest, status: &Status) -> Result<()> {
    for (p, s) in status.iter() {
        match s {
            FileStatus::Deleted | FileStatus::Removed => mf.remove(p).map(|_| ())?,
            FileStatus::Added => mf.insert(
                p.to_owned(),
                FileMetadata {
                    hgid: MF_ADDED_NODE_ID,
                    file_type: file_type(vfs, p),
                },
            )?,
            FileStatus::Modified => mf.insert(
                p.to_owned(),
                FileMetadata {
                    hgid: MF_MODIFIED_NODE_ID,
                    file_type: file_type(vfs, p),
                },
            )?,
            FileStatus::Unknown | FileStatus::Ignored | FileStatus::Clean => (),
        }
    }

    Ok(())
}

pub(crate) fn check_conflicts(
    lgr: &TermLogger,
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    plan: &CheckoutPlan,
    target_mf: &TreeManifest,
    status: &Status,
) -> Result<()> {
    let unknown_conflicts = plan.check_unknown_files(
        target_mf,
        repo.file_store()?.as_ref(),
        &mut wc.treestate().lock(),
        status,
    )?;
    if !unknown_conflicts.is_empty() {
        for unknown in unknown_conflicts {
            lgr.warn(format!("{unknown}: untracked file differs"));
        }
        bail!("untracked files in working directory differ from files in requested revision");
    }

    let conflicts = plan.check_conflicts(&status);
    if !conflicts.is_empty() {
        bail!(
            "{:?} conflicting file changes:\n {}",
            conflicts.len(),
            conflicts
                .iter()
                .take(5)
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n "),
        );
    }
    Ok(())
}

fn create_sparse_matchers(
    repo: &mut Repo,
    vfs: &VFS,
    current_mf: &TreeManifest,
    target_mf: &TreeManifest,
) -> Result<(DynMatcher, Option<(DynMatcher, DynMatcher)>)> {
    let dot_path = repo.dot_hg_path().to_owned();
    if util::file::exists(dot_path.join("sparse"))?.is_none() {
        return Ok((Arc::new(AlwaysMatcher::new()), None));
    }

    let overrides = sparse::config_overrides(repo.config());

    let (current_sparse, current_hash) = sparse::repo_matcher_with_overrides(
        vfs,
        &dot_path,
        current_mf,
        repo.file_store()?,
        &overrides,
    )?
    .unwrap_or_else(|| {
        let matcher: Arc<dyn Matcher + Sync + Send> = Arc::new(AlwaysMatcher::new());
        (matcher, 0)
    });

    let (target_sparse, target_hash) = sparse::repo_matcher_with_overrides(
        vfs,
        &dot_path,
        target_mf,
        repo.file_store()?,
        &overrides,
    )?
    .unwrap_or_else(|| {
        let matcher: Arc<dyn Matcher + Sync + Send> = Arc::new(AlwaysMatcher::new());
        (matcher, 0)
    });

    let sparse_matcher: DynMatcher = Arc::new(UnionMatcher::new(vec![
        current_sparse.clone(),
        target_sparse.clone(),
    ]));
    let sparse_change = if current_hash != target_hash {
        Some((current_sparse, target_sparse))
    } else {
        None
    };

    Ok((sparse_matcher, sparse_change))
}

fn create_plan(
    vfs: &VFS,
    config: &dyn Config,
    current_mf: &TreeManifest,
    target_mf: &TreeManifest,
    matcher: &dyn Matcher,
    sparse_change: Option<(DynMatcher, DynMatcher)>,
    progress_path: Option<PathBuf>,
) -> Result<CheckoutPlan> {
    let diff = Diff::new(current_mf, target_mf, &matcher)?;
    let mut actions = ActionMap::from_diff(diff)?;

    if let Some((old_sparse, new_sparse)) = sparse_change {
        actions =
            actions.with_sparse_profile_change(old_sparse, new_sparse, current_mf, target_mf)?;
    }
    let checkout = Checkout::from_config(vfs.clone(), &config)?;
    let mut plan = checkout.plan_action_map(actions);

    if let Some(progress_path) = progress_path {
        plan.add_progress(&progress_path)?;
    }

    Ok(plan)
}

fn record_updates(plan: &CheckoutPlan, vfs: &VFS, treestate: &mut TreeState) -> Result<()> {
    let bar = ProgressBar::new_adhoc("recording", plan.all_files().count() as u64, "files");

    for removed in plan.removed_files() {
        treestate.remove(removed)?;
        bar.increase_position(1);
    }

    for updated in plan
        .updated_content_files()
        .chain(plan.updated_meta_files())
    {
        let fstate = file_state(vfs, updated)?;
        treestate.insert(updated, &fstate)?;
        bar.increase_position(1);
    }

    Ok(())
}

#[cfg(windows)]
fn is_final_symlink_target_dir(mut path: PathBuf) -> Result<bool> {
    use anyhow::Context;
    // On Linux the usual limit for symlinks depth is 40, and symlinks stop
    // being followed after that point:
    // https://elixir.bootlin.com/linux/v6.5-rc7/source/include/linux/namei.h#L13
    // Let's keep a similar limit for Windows
    let mut rem_links = 40;
    let mut metadata = match fs::symlink_metadata(path.clone()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // The symlink file does not exist. This can happen when writes
            // failed earlier. There should be errors about those writes
            // already. Don't report a different (less readable) error.
            return Ok(false);
        }
        v => v?,
    };
    while metadata.is_symlink() && rem_links > 0 {
        rem_links -= 1;
        let target = fs::read_link(path.clone())?;
        path = path
            .parent()
            .context("unable to determine parent directory for path when resolving symlink")?
            .to_owned();
        path.push(target);
        if !path.exists() {
            // If final target doesn't exist report it as a regular file
            return Ok(false);
        }
        metadata = fs::symlink_metadata(path.clone())?;
    }
    Ok(metadata.is_dir())
}

#[cfg(windows)]
/// Converts a list of file symlinks into potentially directory symlinks by
/// checking the final target of that symlink, and converting it into a
/// directory one if the final target is a directory.
pub fn update_symlinks(paths: &[&RepoPath], vfs: &VFS) -> Result<()> {
    use anyhow::Context;
    for p in paths {
        let path = RepoPath::from_str(p.as_str())?;
        if is_final_symlink_target_dir(vfs.join(path))? {
            let (contents, _) = vfs.read_with_metadata(&path)?;
            let target = PathBuf::from(String::from_utf8(contents.into_vec())?);
            let target = util::path::replace_slash_with_backslash(&target);
            let path = vfs.join(path);
            util::path::remove_file(&path).context("Unable to remove symlink")?;
            util::path::symlink_dir(&target, &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
// todo - consider moving some of this code to vfs / separate test create
// todo parallel execution for the test
mod test {
    use std::collections::HashMap;
    use std::path::Path;

    #[cfg(unix)]
    use anyhow::ensure;
    use anyhow::Context;
    use fs::create_dir;
    #[cfg(unix)]
    use fs_err as fs;
    use manifest_tree::testutil::make_tree_manifest_from_meta;
    use manifest_tree::testutil::TestStore;
    use manifest_tree::Diff;
    use pathmatcher::AlwaysMatcher;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use storemodel::KeyStore;
    use tempfile::TempDir;
    use types::testutil::generate_repo_paths;
    use walkdir::DirEntry;
    use walkdir::WalkDir;

    use super::*;

    #[test]
    fn test_basic_checkout() -> Result<()> {
        // Pattern - lowercase_path_[hgid!=1]_[flags!=normal]
        let a = (rp("A"), FileMetadata::regular(hgid(1)));
        let a_2 = (rp("A"), FileMetadata::regular(hgid(2)));
        let a_e = (rp("A"), FileMetadata::executable(hgid(1)));
        let a_s = (rp("A"), FileMetadata::symlink(hgid(1)));
        let b = (rp("B"), FileMetadata::regular(hgid(1)));
        let ab = (rp("A/B"), FileMetadata::regular(hgid(1)));
        let cd = (rp("C/D"), FileMetadata::regular(hgid(1)));

        // update file
        assert_checkout(&[a.clone()], &[a_2.clone()])?;
        // mv file
        assert_checkout(&[a.clone()], &[b.clone()])?;
        // add / rm file
        assert_checkout_symmetrical(&[a.clone()], &[a.clone(), b.clone()])?;
        // regular<->exec
        assert_checkout_symmetrical(&[a.clone()], &[a_e.clone()])?;
        // regular<->symlink
        assert_checkout_symmetrical(&[a.clone()], &[a_s.clone()])?;
        // dir <-> file with the same name
        assert_checkout_symmetrical(&[ab.clone()], &[a.clone()])?;
        // create / rm dir
        assert_checkout_symmetrical(&[ab.clone()], &[b.clone()])?;
        // mv file between dirs
        assert_checkout(&[ab.clone()], &[cd.clone()])?;

        Ok(())
    }

    #[test]
    fn test_checkout_generated() -> Result<()> {
        let trees = generate_trees(6, 50);
        for a in trees.iter() {
            for b in trees.iter() {
                if a == b {
                    continue;
                }
                assert_checkout(a, b)?;
            }
        }
        Ok(())
    }

    #[test]
    fn test_progress_parsing() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let working_path = tempdir.path().to_path_buf().join("workingdir");
        create_dir(working_path.as_path()).unwrap();
        let vfs = VFS::new(working_path)?;
        let path = tempdir.path().to_path_buf().join("updateprogress");
        let mut progress = CheckoutProgress::new(&path, vfs.clone())?;
        let file_path = RepoPathBuf::from_string("file".to_string())?;
        vfs.write(file_path.as_repo_path(), &[0b0, 0b01], UpdateFlag::Regular)?;
        let id = hgid(1);
        progress.record_writes(&[(id, file_path.clone())]);

        let progress = CheckoutProgress::load(&path, vfs)?;
        assert_eq!(progress.state.len(), 1);
        assert_eq!(progress.state.get(&file_path).unwrap().0, id);
        Ok(())
    }

    fn generate_trees(tree_size: usize, count: usize) -> Vec<Vec<(RepoPathBuf, FileMetadata)>> {
        let mut result = vec![];
        let mut gen = Gen::new(5);
        let paths = generate_repo_paths(tree_size * count, &mut gen);

        for i in 0..count {
            let mut tree = vec![];
            for idx in 0..tree_size {
                let meta = FileMetadata::arbitrary(&mut gen);
                let path = paths.get(i * tree_size / 2 + idx).unwrap().clone();
                tree.push((path, meta));
            }
            result.push(tree)
        }
        result
    }

    fn rp(p: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(p.to_string()).unwrap()
    }

    fn hgid(p: u8) -> HgId {
        let mut r = HgId::default().into_byte_array();
        r[0] = p;
        HgId::from_byte_array(r)
    }

    fn assert_checkout_symmetrical(
        a: &[(RepoPathBuf, FileMetadata)],
        b: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        assert_checkout(a, b)?;
        assert_checkout(b, a)
    }

    fn assert_checkout(
        from: &[(RepoPathBuf, FileMetadata)],
        to: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        if let Err(e) = assert_checkout_impl(from, to, &tempdir) {
            eprintln!("===");
            eprintln!("Failed transitioning from tree");
            print_tree(from);
            eprintln!("To tree");
            print_tree(to);
            eprintln!("===");
            eprintln!(
                "Working directory: {} (not deleted)",
                tempdir.into_path().display()
            );
            return Err(e);
        }
        Ok(())
    }

    fn assert_checkout_impl(
        from: &[(RepoPathBuf, FileMetadata)],
        to: &[(RepoPathBuf, FileMetadata)],
        tempdir: &TempDir,
    ) -> Result<()> {
        let working_path = tempdir.path().to_path_buf().join("workingdir");
        create_dir(working_path.as_path()).unwrap();
        let vfs = VFS::new(working_path.clone())?;
        roll_out_fs(&vfs, from)?;

        let store = Arc::new(TestStore::new());
        let matcher = AlwaysMatcher::new();
        let left_tree = make_tree_manifest_from_meta(store.clone(), from.iter().cloned());
        let right_tree = make_tree_manifest_from_meta(store, to.iter().cloned());
        let diff = Diff::new(&left_tree, &right_tree, &matcher).unwrap();
        let vfs = VFS::new(working_path.clone())?;
        let checkout = Checkout::default_config(vfs);
        let plan = checkout
            .plan_action_map(ActionMap::from_diff(diff).context("Plan construction failed")?);

        // Use clean vfs for test
        plan.apply_store(&DummyFileContentStore)
            .context("Plan execution failed")?;

        assert_fs(&working_path, to)
    }

    fn print_tree(t: &[(RepoPathBuf, FileMetadata)]) {
        for (path, meta) in t {
            eprintln!("{} [{:?}]", path, meta);
        }
    }

    fn roll_out_fs(vfs: &VFS, files: &[(RepoPathBuf, FileMetadata)]) -> Result<()> {
        for (path, meta) in files {
            let flag = type_to_flag(&meta.file_type);
            let data = hgid_file(&meta.hgid);
            vfs.write(path.as_repo_path(), &data, flag)?;
        }
        Ok(())
    }

    fn assert_fs(root: &Path, expected: &[(RepoPathBuf, FileMetadata)]) -> Result<()> {
        let mut expected: HashMap<_, _> = expected.iter().cloned().collect();
        for dir in WalkDir::new(root).into_iter() {
            let dir = dir?;
            if dir.file_type().is_dir() {
                assert_not_empty_dir(&dir)?;
                continue;
            }
            let rel_path = dir.path().strip_prefix(root)?;
            let rel_path = into_repo_path(rel_path.to_string_lossy().into_owned());
            let rel_path = RepoPathBuf::from_string(rel_path)?;
            let expected_meta = if let Some(m) = expected.remove(&rel_path) {
                m
            } else {
                bail!("Checkout created unexpected file {}", rel_path);
            };
            assert_metadata(&expected_meta, &dir)?;
        }
        if !expected.is_empty() {
            bail!(
                "Some files are not present after checkout: {:?}",
                expected.keys().collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn into_repo_path(path: String) -> String {
        path
    }

    #[cfg(windows)]
    fn into_repo_path(path: String) -> String {
        path.replace("\\", "/")
    }

    fn assert_not_empty_dir(dir: &DirEntry) -> Result<()> {
        let mut rd = fs::read_dir(dir.path())?;
        if rd.next().is_none() {
            bail!("Unexpected empty dir: {}", dir.path().display())
        }
        Ok(())
    }

    fn assert_metadata(expected: &FileMetadata, actual: &DirEntry) -> Result<()> {
        match expected.file_type {
            FileType::Regular => assert_regular(actual),
            FileType::Executable => assert_exec(actual),
            FileType::Symlink => assert_symlink(actual),
            FileType::GitSubmodule => Ok(()),
        }
    }

    // When compiling on unknown platform will get function not defined compile error and will need to address it

    #[cfg(unix)] // This is where PermissionsExt is defined
    fn assert_regular(actual: &DirEntry) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let meta = actual.metadata()?;
        ensure!(
            meta.permissions().mode() & 0o111 == 0,
            "Expected {} to be a regular file, actual mode {:#o}",
            actual.path().display(),
            meta.permissions().mode()
        );
        Ok(())
    }

    #[cfg(unix)]
    fn assert_exec(actual: &DirEntry) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let meta = actual.metadata()?;
        ensure!(
            meta.permissions().mode() & 0o111 != 0,
            "Expected {} to be a executable file, actual mode {:#o}",
            actual.path().display(),
            meta.permissions().mode()
        );
        Ok(())
    }

    #[cfg(unix)]
    fn assert_symlink(actual: &DirEntry) -> Result<()> {
        ensure!(
            actual.path_is_symlink(),
            "Expected {} to be a symlink",
            actual.path().display()
        );
        Ok(())
    }

    #[cfg(windows)]
    fn assert_regular(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn assert_exec(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn assert_symlink(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    struct DummyFileContentStore;

    #[async_trait::async_trait]
    impl KeyStore for DummyFileContentStore {
        fn get_local_content(&self, _path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Bytes>> {
            Ok(Some(hgid_file(&hgid).into()))
        }
    }

    #[async_trait::async_trait]
    impl FileStore for DummyFileContentStore {}

    fn hgid_file(hgid: &HgId) -> Vec<u8> {
        hgid.to_string().into_bytes()
    }
}
