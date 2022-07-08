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
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_runtime::try_block_unless_interrupted as block_on;
use futures::stream;
use futures::try_join;
use futures::Stream;
use futures::StreamExt;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use minibytes::Bytes;
use parking_lot::Mutex;
use progress_model::ProgressBar;
use progress_model::Registry;
use storemodel::ReadFileContents;
use tracing::debug;
use tracing::instrument;
use tracing::warn;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::AsyncVfsWriter;
use vfs::UpdateFlag;
use vfs::VFS;

#[allow(dead_code)]
mod actions;
pub mod clone;
#[allow(dead_code)]
mod conflict;
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
use tokio::runtime::Handle;

const VFS_BATCH_SIZE: usize = 100;

/// Contains lists of files to be removed / updated during checkout.
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files that needs their content updated.
    update_content: Vec<UpdateContentAction>,
    filtered_update_content: Vec<UpdateContentAction>,
    /// Files that only need X flag updated.
    update_meta: Vec<UpdateMetaAction>,
    progress: Option<Mutex<CheckoutProgress>>,
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
    /// Path to file.
    path: RepoPathBuf,
    /// If content has changed, HgId of new content.
    content_hgid: HgId,
    /// New file type.
    file_type: FileType,
    /// Whether this is a new file.
    new_file: bool,
}

/// Only update metadata on the file, do not update content
#[derive(Debug)]
struct UpdateMetaAction {
    /// Path to file.
    path: RepoPathBuf,
    /// true if need to set executable flag, false if need to remove it.
    set_x_flag: bool,
}

#[derive(Default)]
pub struct CheckoutStats {
    removed: AtomicUsize,
    updated: AtomicUsize,
    meta_updated: AtomicUsize,
    written_bytes: AtomicUsize,
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
        let mut update_content = vec![];
        let mut update_meta = vec![];
        for (path, action) in map.into_iter() {
            match action {
                Action::Remove => remove.push(path),
                Action::UpdateExec(set_x_flag) => {
                    update_meta.push(UpdateMetaAction { path, set_x_flag })
                }
                Action::Update(up) => {
                    update_content.push(UpdateContentAction::new(path, up.to, up.from.is_none()))
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
            match CheckoutProgress::load(path, vfs.clone()) {
                Ok(p) => p,
                Err(e) => {
                    debug!("Failed to load CheckoutProgress with {:?}", e);
                    CheckoutProgress::new(path, vfs.clone())?
                }
            }
        } else {
            CheckoutProgress::new(path, vfs.clone())?
        };
        self.filtered_update_content = progress.filter_already_written(&self.update_content);
        self.progress = Some(Mutex::new(progress));
        Ok(())
    }

    /// Applies plan to the root using store to fetch data.
    /// This async function offloads file system operation to tokio blocking thread pool.
    /// It limits number of concurrent fs operations to Checkout::concurrency.
    ///
    /// This function also designed to leverage async storage API.
    /// When updating content of the file/symlink, this function first creates list of HgId
    /// it needs to fetch. This list is then converted to stream and fed into storage for fetching
    ///
    /// As storage starts returning blobs of data, we start to kick off fs write operations in
    /// the tokio async worker pool. If more then Checkout::concurrency fs operations are pending, we
    /// stop polling storage stream, until one of pending fs operations complete
    ///
    /// This function fails fast and returns error when first checkout operation fails.
    /// Pending storage futures are dropped when error is returned
    pub async fn apply_store(
        &self,
        store: &dyn ReadFileContents<Error = anyhow::Error>,
    ) -> Result<CheckoutStats> {
        let vfs = &self.checkout.vfs;
        debug!(
            "Skipping checking out {} files since they're already written",
            self.update_content.len() - self.filtered_update_content.len()
        );
        let total = self.filtered_update_content.len() + self.remove.len() + self.update_meta.len();
        let bar = &ProgressBar::new("Updating", total as u64, "files");
        Registry::main().register_progress_bar(bar);
        let async_vfs = &AsyncVfsWriter::spawn_new(vfs.clone(), 16);
        let stats = CheckoutStats::default();
        let stats_ref = &stats;

        let remove_files = stream::iter(self.remove.clone().into_iter())
            .chunks(VFS_BATCH_SIZE)
            .map(|paths| Self::remove_files(async_vfs, stats_ref, paths, bar));
        let remove_files = remove_files.buffer_unordered(self.checkout.concurrency);

        Self::process_work_stream(remove_files).await?;

        let actions: HashMap<_, _> = self
            .filtered_update_content
            .iter()
            .map(|u| (u.make_key(), u.clone()))
            .collect();
        let keys: Vec<_> = actions.keys().cloned().collect();

        let data_stream = store.read_file_contents(keys).await;

        let update_content = data_stream.map(|result| -> Result<_> {
            let (data, key) = result?;
            let action = actions
                .get(&key)
                .ok_or_else(|| format_err!("Storage returned unknown key {}", key))?;
            let path = action.path.clone();
            let flag = type_to_flag(&action.file_type);
            Ok((path, action.content_hgid, data, flag))
        });

        let progress_ref = self.progress.as_ref();
        let update_content = update_content
            .chunks(VFS_BATCH_SIZE)
            .map(|actions| async move {
                let actions: Result<Vec<_>, _> = actions.into_iter().collect();
                Self::write_files(async_vfs, stats_ref, actions?, progress_ref, bar).await
            });

        let update_content = update_content.buffer_unordered(self.checkout.concurrency);

        let update_meta = stream::iter(self.update_meta.iter()).map(|action| {
            Self::set_exec_on_file(async_vfs, stats_ref, &action.path, action.set_x_flag, bar)
        });
        let update_meta = update_meta.buffer_unordered(self.checkout.concurrency);

        let update_content = Self::process_work_stream(update_content);
        let update_meta = Self::process_work_stream(update_meta);

        try_join!(update_content, update_meta)?;

        Ok(stats)
    }

    #[instrument(skip_all, err)]
    pub fn blocking_apply_store(
        &self,
        store: &dyn ReadFileContents<Error = anyhow::Error>,
    ) -> Result<CheckoutStats> {
        block_on(self.apply_store(store))
    }

    pub async fn apply_store_dry_run(
        &self,
        store: &dyn ReadFileContents<Error = anyhow::Error>,
    ) -> Result<(usize, u64)> {
        let keys = self
            .filtered_update_content
            .iter()
            .map(UpdateContentAction::make_key);
        let mut stream = store.read_file_contents(keys.collect()).await;
        let (mut count, mut size) = (0, 0);
        while let Some(result) = stream.next().await {
            let (bytes, _) = result?;
            count += 1;
            size += bytes.len() as u64;
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

    pub async fn check_unknown_files(
        &self,
        manifest: &impl Manifest,
        store: &dyn ReadFileContents<Error = anyhow::Error>,
        tree_state: &mut TreeState,
        status: &Status,
    ) -> Result<Vec<RepoPathBuf>> {
        let vfs = &self.checkout.vfs;
        let mut check_content = vec![];

        let new_files: Vec<_> = self.new_file_actions().collect();

        let bar = ProgressBar::register_new("Checking untracked", new_files.len() as u64, "files");
        for file_action in new_files {
            let file: &RepoPathBuf = &file_action.path;

            bar.increase_position(1);
            if !matches!(status.status(file), Some(FileStatus::Unknown)) {
                continue;
            }
            bar.set_message(file.to_string());

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

        let check_content = store
            .read_file_contents(check_content)
            .await
            .chunks(VFS_BATCH_SIZE)
            .map(|v| {
                let vfs = vfs.clone();
                Handle::current().spawn_blocking(move || -> Result<Vec<RepoPathBuf>> {
                    let v: std::result::Result<Vec<_>, _> = v.into_iter().collect();
                    Self::check_content(&vfs, v?)
                })
            })
            .buffer_unordered(self.checkout.concurrency)
            .map(|r| r?);

        let unknowns = Self::process_vec_work_stream(check_content).await?;

        Ok(unknowns)
    }

    /// Drains stream returning error if one of futures fail
    async fn process_work_stream<S: Stream<Item = Result<()>> + Unpin>(
        mut stream: S,
    ) -> Result<()> {
        while let Some(result) = stream.next().await {
            result?;
        }
        Ok(())
    }

    async fn process_vec_work_stream<R: Send, S: Stream<Item = Result<Vec<R>>> + Unpin>(
        mut stream: S,
    ) -> Result<Vec<R>> {
        let mut r = vec![];
        while let Some(result) = stream.next().await {
            r.append(&mut result?);
        }
        Ok(r)
    }

    fn check_content(vfs: &VFS, files: Vec<(Bytes, Key)>) -> Result<Vec<RepoPathBuf>> {
        let mut result = vec![];
        for file in files {
            let path = &file.1.path;
            match Self::check_file(vfs, file.0, path) {
                Err(err) => {
                    warn!("Can not check {}: {}", path, err);
                    result.push(path.clone())
                }
                Ok(false) => result.push(path.clone()),
                Ok(true) => {}
            }
        }
        Ok(result)
    }

    fn check_file(vfs: &VFS, expected_content: Bytes, path: &RepoPath) -> Result<bool> {
        let actual_content = vfs.read(path)?;
        Ok(actual_content.eq(&expected_content))
    }

    // Functions below use blocking fs operations in spawn_blocking proc.
    // As of today tokio::fs operations do the same.
    // Since we do multiple fs calls inside, it is beneficial to 'pack'
    // all of them into single spawn_blocking.
    async fn write_files(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        actions: Vec<(RepoPathBuf, HgId, Bytes, UpdateFlag)>,
        progress: Option<&Mutex<CheckoutProgress>>,
        bar: &Arc<ProgressBar>,
    ) -> Result<()> {
        let count = actions.len();

        let first_file = actions
            .get(0)
            .expect("Cant have empty actions in write_files")
            .0
            .to_string();
        bar.set_message(first_file);

        let paths: Vec<_> = actions
            .iter()
            .map(|(path, hgid, _, _)| (hgid.clone(), path.as_repo_path().to_owned()))
            .collect();
        let actions = actions
            .into_iter()
            .map(|(path, _, content, flag)| (path, content, flag));
        let w = async_vfs.write_batch(actions).await?;
        stats.updated.fetch_add(count, Ordering::Relaxed);
        stats.written_bytes.fetch_add(w, Ordering::Relaxed);

        if let Some(progress) = progress {
            progress.lock().record_writes(paths);
            fail::fail_point!("checkout-post-progress", |_| { bail!("oh no!") });
        }
        bar.increase_position(count as u64);

        Ok(())
    }

    async fn remove_files(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        paths: Vec<RepoPathBuf>,
        bar: &Arc<ProgressBar>,
    ) -> Result<()> {
        let count = paths.len();
        async_vfs.remove_batch(paths).await?;
        stats.removed.fetch_add(count, Ordering::Relaxed);
        bar.increase_position(count as u64);
        Ok(())
    }

    async fn set_exec_on_file(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        path: &RepoPath,
        flag: bool,
        bar: &Arc<ProgressBar>,
    ) -> Result<()> {
        async_vfs
            .set_executable(path.to_owned(), flag)
            .await
            .context(format!("Updating exec on {}", path))?;
        stats.meta_updated.fetch_add(1, Ordering::Relaxed);
        bar.increase_position(1);
        Ok(())
    }

    pub fn removed_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.remove.iter()
    }

    pub fn updated_content_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_content.iter().map(|u| &u.path)
    }

    pub fn updated_meta_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_meta.iter().map(|u| &u.path)
    }

    fn new_file_actions(&self) -> impl Iterator<Item = &UpdateContentAction> {
        // todo - index new files so that this function don't need to be O(total_files_changed)test-update-names.t.err
        self.filtered_update_content.iter().filter(|u| u.new_file)
    }

    pub fn all_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_content
            .iter()
            .map(|u| &u.path)
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
            update_content: vec![],
            filtered_update_content: vec![],
            update_meta: vec![],
            progress: None,
            checkout: Checkout::default_config(vfs),
        }
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
    ///
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

    fn record_writes(&mut self, paths: Vec<(HgId, RepoPathBuf)>) {
        for (hgid, path) in paths.into_iter() {
            // Don't report write failures, just let the checkout continue.
            let _ = (|| -> Result<()> {
                let stat = self.vfs.metadata(&path)?;
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
        actions: &[UpdateContentAction],
    ) -> Vec<UpdateContentAction> {
        // TODO: This should be done in parallel. Maybe with the new vfs async batch APIs?
        let bar = ProgressBar::register_new("Filtering existing", actions.len() as u64, "files");
        actions
            .iter()
            .filter(move |action| {
                let path = &action.path;
                if let Some((hgid, time, size)) = &self.state.get(path) {
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
            .map(|a| a.clone())
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
    pub fn new(path: RepoPathBuf, meta: FileMetadata, new_file: bool) -> Self {
        Self {
            path,
            content_hgid: meta.hgid,
            file_type: meta.file_type,
            new_file,
        }
    }

    pub fn make_key(&self) -> Key {
        Key::new(self.path.clone(), self.content_hgid)
    }
}

impl AsRef<RepoPath> for UpdateContentAction {
    fn as_ref(&self) -> &RepoPath {
        &self.path
    }
}

impl AsRef<RepoPath> for UpdateMetaAction {
    fn as_ref(&self) -> &RepoPath {
        &self.path
    }
}

#[cfg(test)]
// todo - consider moving some of this code to vfs / separate test create
// todo parallel execution for the test
mod test {
    use std::collections::HashMap;
    use std::fs::create_dir;
    use std::path::Path;

    use anyhow::ensure;
    use anyhow::Context;
    use futures::stream::BoxStream;
    use manifest_tree::testutil::make_tree_manifest_from_meta;
    use manifest_tree::testutil::TestStore;
    use manifest_tree::Diff;
    use pathmatcher::AlwaysMatcher;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use tempfile::TempDir;
    use types::testutil::generate_repo_paths;
    use walkdir::DirEntry;
    use walkdir::WalkDir;

    use super::*;

    #[tokio::test]
    async fn test_basic_checkout() -> Result<()> {
        // Pattern - lowercase_path_[hgid!=1]_[flags!=normal]
        let a = (rp("A"), FileMetadata::regular(hgid(1)));
        let a_2 = (rp("A"), FileMetadata::regular(hgid(2)));
        let a_e = (rp("A"), FileMetadata::executable(hgid(1)));
        let a_s = (rp("A"), FileMetadata::symlink(hgid(1)));
        let b = (rp("B"), FileMetadata::regular(hgid(1)));
        let ab = (rp("A/B"), FileMetadata::regular(hgid(1)));
        let cd = (rp("C/D"), FileMetadata::regular(hgid(1)));

        // update file
        assert_checkout(&[a.clone()], &[a_2.clone()]).await?;
        // mv file
        assert_checkout(&[a.clone()], &[b.clone()]).await?;
        // add / rm file
        assert_checkout_symmetrical(&[a.clone()], &[a.clone(), b.clone()]).await?;
        // regular<->exec
        assert_checkout_symmetrical(&[a.clone()], &[a_e.clone()]).await?;
        // regular<->symlink
        assert_checkout_symmetrical(&[a.clone()], &[a_s.clone()]).await?;
        // dir <-> file with the same name
        assert_checkout_symmetrical(&[ab.clone()], &[a.clone()]).await?;
        // create / rm dir
        assert_checkout_symmetrical(&[ab.clone()], &[b.clone()]).await?;
        // mv file between dirs
        assert_checkout(&[ab.clone()], &[cd.clone()]).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_checkout_generated() -> Result<()> {
        let trees = generate_trees(6, 50);
        for a in trees.iter() {
            for b in trees.iter() {
                if a == b {
                    continue;
                }
                assert_checkout(a, b).await?;
            }
        }
        Ok(())
    }

    #[test]
    fn test_progress_parsing() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let working_path = tempdir.path().to_path_buf().join("workingdir");
        create_dir(working_path.as_path()).unwrap();
        let vfs = VFS::new(working_path.clone())?;
        let path = tempdir.path().to_path_buf().join("updateprogress");
        let mut progress = CheckoutProgress::new(&path, vfs.clone())?;
        let file_path = RepoPathBuf::from_string("file".to_string())?;
        vfs.write(file_path.as_repo_path(), &[0b0, 0b01], UpdateFlag::Regular)?;
        let id = hgid(1);
        progress.record_writes(vec![(id, file_path.clone())]);

        let progress = CheckoutProgress::load(&path, vfs.clone())?;
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

    async fn assert_checkout_symmetrical(
        a: &[(RepoPathBuf, FileMetadata)],
        b: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        assert_checkout(a, b).await?;
        assert_checkout(b, a).await
    }

    async fn assert_checkout(
        from: &[(RepoPathBuf, FileMetadata)],
        to: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        if let Err(e) = assert_checkout_impl(from, to, &tempdir).await {
            eprintln!("===");
            eprintln!("Failed transitioning from tree");
            print_tree(&from);
            eprintln!("To tree");
            print_tree(&to);
            eprintln!("===");
            eprintln!(
                "Working directory: {} (not deleted)",
                tempdir.into_path().display()
            );
            return Err(e);
        }
        Ok(())
    }

    async fn assert_checkout_impl(
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
            .await
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
        let mut rd = std::fs::read_dir(dir.path())?;
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
    impl ReadFileContents for DummyFileContentStore {
        type Error = anyhow::Error;

        async fn read_file_contents(&self, keys: Vec<Key>) -> BoxStream<Result<(Bytes, Key)>> {
            stream::iter(keys)
                .map(|key| Ok((hgid_file(&key.hgid).into(), key)))
                .boxed()
        }
    }

    fn hgid_file(hgid: &HgId) -> Vec<u8> {
        hgid.to_string().into_bytes()
    }
}

impl fmt::Display for CheckoutPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.remove {
            writeln!(f, "rm {}", r)?;
        }
        for u in &self.update_content {
            let ft = match u.file_type {
                FileType::Executable => "(x)",
                FileType::Symlink => "(s)",
                FileType::Regular => "",
                FileType::GitSubmodule => continue,
            };
            writeln!(f, "up {}=>{}{}", u.path, u.content_hgid, ft)?;
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
    let mode = 0o644; // todo figure this out
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
