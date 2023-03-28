/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Receiver;
use crossbeam::channel::SendError;
use crossbeam::channel::Sender;
use futures::StreamExt;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use parking_lot::RwLock;
use pathmatcher::ExactMatcher;
use progress_model::ProgressBar;
use storemodel::minibytes::Bytes;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use types::Key;
use types::RepoPathBuf;
use vfs::VFS;

use crate::filesystem::ChangeType;
use crate::metadata;
use crate::metadata::HgModifiedTime;
use crate::metadata::Metadata;

pub type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;

pub(crate) enum FileChangeResult {
    Yes(ChangeType),
    No(RepoPathBuf),
    Maybe((RepoPathBuf, Metadata)),
}

impl FileChangeResult {
    fn changed(path: RepoPathBuf) -> Self {
        Self::Yes(ChangeType::Changed(path))
    }

    fn deleted(path: RepoPathBuf) -> Self {
        Self::Yes(ChangeType::Deleted(path))
    }
}

#[derive(Debug)]
pub enum ResolvedFileChangeResult {
    Yes(ChangeType),
    No(RepoPathBuf),
}

impl ResolvedFileChangeResult {
    fn changed(path: RepoPathBuf) -> Self {
        Self::Yes(ChangeType::Changed(path))
    }
}

pub(crate) trait FileChangeDetectorTrait:
    IntoIterator<Item = Result<ResolvedFileChangeResult>>
{
    fn submit(&mut self, file: metadata::File);
    fn total_work_hint(&self, _hint: u64) {}
}

pub struct FileChangeDetector {
    vfs: VFS,
    last_write: HgModifiedTime,
    results: Vec<Result<ResolvedFileChangeResult>>,
    lookups: RepoPathMap<Metadata>,
    manifest: Arc<RwLock<TreeManifest>>,
    store: ArcReadFileContents,
}

impl FileChangeDetector {
    pub fn new(
        vfs: VFS,
        last_write: HgModifiedTime,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
    ) -> Self {
        let case_sensitive = vfs.case_sensitive();
        FileChangeDetector {
            vfs,
            last_write,
            lookups: RepoPathMap::new(case_sensitive),
            results: Vec::new(),
            manifest,
            store,
        }
    }
}

const NEED_CHECK: StateFlags = StateFlags::NEED_CHECK;
const EXIST_P1: StateFlags = StateFlags::EXIST_P1;

pub(crate) fn file_changed_given_metadata(
    vfs: &VFS,
    file: metadata::File,
    last_write: HgModifiedTime,
) -> Result<FileChangeResult> {
    let path = file.path;

    // First handle when metadata is None (i.e. file doesn't exist).
    let (fs_meta, state) = match (file.fs_meta, file.ts_state) {
        // File was untracked during crawl but no longer exists.
        (None, None) => {
            tracing::trace!(?path, "neither on disk nor in treestate");
            return Ok(FileChangeResult::No(path));
        }

        // File was not found but exists in P1: mark as deleted.
        (None, Some(state)) if state.state.intersects(EXIST_P1) => {
            tracing::trace!(?path, "not on disk, in P1");
            return Ok(FileChangeResult::deleted(path.to_owned()));
        }

        // File doesn't exist, isn't in P1 but exists in treestate.
        // This can happen when watchman is tracking that this file needs
        // checking for example.
        (None, Some(_)) => {
            tracing::trace!(?path, "neither on disk nor in P1");
            return Ok(FileChangeResult::No(path));
        }

        (Some(m), s) => (m, s),
    };

    // Don't check EXIST_P2. If file is only in P2 we want to report "changed"
    // even if its contents happen to match an untracked file on disk.
    let in_parent = matches!(&state, Some(s) if s.state.intersects(EXIST_P1));
    let is_trackable_file = fs_meta.is_file(vfs) || fs_meta.is_symlink(vfs);

    let state = match (in_parent, is_trackable_file) {
        // If the file is not valid (e.g. a directory or a weird file like
        // a fifo file) but exists in P1 (as a valid file at some previous
        // time) then we consider it now deleted.
        (true, false) => {
            tracing::trace!(?path, "changed (in_parent, !trackable)");
            return Ok(FileChangeResult::deleted(path));
        }
        // File not in parent and not trackable - skip it. We can get here if
        // the file was valid during the crawl but no longer is.
        (false, false) => {
            tracing::trace!(?path, "no (!in_parent, !trackable)");
            return Ok(FileChangeResult::No(path));
        }
        // File exists but is not in the treestate (untracked)
        (false, true) => {
            tracing::trace!(?path, "changed (!in_parent, trackable)");
            return Ok(FileChangeResult::changed(path));
        }
        (true, true) => state.unwrap(),
    };

    let flags = state.state;

    let ts_meta: Metadata = state.into();

    // If working copy file size or flags are different from what is in treestate, it has changed.
    // Note: state.size is i32 since Mercurial uses negative numbers to indicate special files.
    // A -1 indicates the file is either in a merge state or a lookup state.
    // A -2 indicates the file comes from the other parent (and may or may not exist in the
    // current parent).
    //
    // Regardless, if the size is negative, we'll do a lookup comparison since we can't
    // determine if the file has changed relative to p1. This logic is a mess and we should get
    // rid of all these negative numbers.
    if let Some(ts_size) = ts_meta.len() {
        let size_different = fs_meta.len() != Some(ts_size);
        let exec_different = fs_meta.is_executable(vfs) != ts_meta.is_executable(vfs);
        let symlink_different = fs_meta.is_symlink(vfs) != ts_meta.is_symlink(vfs);
        if size_different || exec_different || symlink_different {
            tracing::trace!(
                ?path,
                size_different,
                exec_different,
                symlink_different,
                "changed (metadata mismatch)"
            );
            return Ok(FileChangeResult::changed(path.to_owned()));
        }
    } else {
        tracing::trace!(?path, "maybe (no size)");
        return Ok(FileChangeResult::Maybe((path, fs_meta)));
    }

    // If it's marked NEED_CHECK, we always need to do a lookup, regardless of the mtime.
    if flags.intersects(NEED_CHECK) {
        tracing::trace!(?path, "maybe (NEED_CHECK)");
        return Ok(FileChangeResult::Maybe((path, fs_meta)));
    }

    // If the mtime has changed or matches the last normal() write time, we need to compare the
    // file contents in the later Lookups phase.  mtime can be negative as well. A -1 indicates
    // the file is in a lookup state. Since a -1 will always cause the equality comparison
    // below to fail and force a lookup, the -1 is handled correctly without special casing. In
    // theory all -1 files should be marked NEED_CHECK above (I think).
    let ts_mtime = match ts_meta.mtime() {
        None => {
            tracing::trace!(?path, "maybe (no mtime)");
            return Ok(FileChangeResult::Maybe((path, fs_meta)));
        }
        Some(ts) => ts,
    };

    if Some(ts_mtime) != fs_meta.mtime() || ts_mtime == last_write {
        tracing::trace!(?path, "maybe (mtime doesn't match)");
        return Ok(FileChangeResult::Maybe((path, fs_meta)));
    }

    tracing::trace!(?path, "no (fallthrough)");
    Ok(FileChangeResult::No(path))
}

fn compare_repo_bytes_to_disk(
    vfs: &VFS,
    repo_bytes: Bytes,
    path: RepoPathBuf,
) -> Result<ResolvedFileChangeResult> {
    match vfs.read(&path) {
        Ok(disk_bytes) => {
            if disk_bytes == repo_bytes {
                tracing::trace!(?path, "no (contents match)");
                Ok(ResolvedFileChangeResult::No(path))
            } else {
                tracing::trace!(?path, "changed (contents mismatch)");
                Ok(ResolvedFileChangeResult::Yes(ChangeType::Changed(path)))
            }
        }
        Err(e) => {
            if let Some(e) = e.downcast_ref::<std::io::Error>() {
                if e.kind() == std::io::ErrorKind::NotFound {
                    tracing::trace!(?path, "deleted (file missing)");
                    return Ok(ResolvedFileChangeResult::Yes(ChangeType::Deleted(path)));
                }
            }

            if let Some(vfs::AuditError::ThroughSymlink(_)) = e.downcast_ref::<vfs::AuditError>() {
                tracing::trace!(?path, "deleted (read through symlink)");
                return Ok(ResolvedFileChangeResult::Yes(ChangeType::Deleted(path)));
            }

            Err(e)
        }
    }
}

impl FileChangeDetector {
    pub(crate) fn has_changed_with_fresh_metadata(
        &mut self,
        file: metadata::File,
    ) -> Result<FileChangeResult> {
        let res = file_changed_given_metadata(&self.vfs, file, self.last_write);

        if let Ok(FileChangeResult::Maybe((ref path, ref meta))) = res {
            self.lookups.insert(path.to_owned(), meta.clone());
        }

        res
    }
}

impl FileChangeDetectorTrait for FileChangeDetector {
    fn submit(&mut self, mut file: metadata::File) {
        if file.fs_meta.is_none() {
            file.fs_meta = match self.vfs.metadata(&file.path) {
                Ok(metadata) => Some(metadata.into()),
                Err(e) => match e.downcast_ref::<std::io::Error>() {
                    Some(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    _ => {
                        self.results.push(Err(e));
                        return;
                    }
                },
            };
        }

        match self.has_changed_with_fresh_metadata(file) {
            Ok(res) => match res {
                FileChangeResult::Yes(change) => {
                    self.results.push(Ok(ResolvedFileChangeResult::Yes(change)))
                }
                FileChangeResult::No(path) => {
                    self.results.push(Ok(ResolvedFileChangeResult::No(path)))
                }
                FileChangeResult::Maybe((path, meta)) => {
                    self.lookups.insert(path, meta);
                }
            },
            Err(err) => self.results.push(Err(err)),
        };
    }
}

fn manifest_flags_mismatch(vfs: &VFS, mf_meta: Metadata, fs_meta: &Metadata) -> bool {
    mf_meta.is_symlink(vfs) != fs_meta.is_symlink(vfs)
        || mf_meta.is_executable(vfs) != fs_meta.is_executable(vfs)
}

// Allows case insensitive tracking of RepoPathBuf->V. We need this because we
// "lose" the caseness of a path after it goes through
// manifest.files(ExectMatcher::new([path], case_sensitive=true)). The manifest
// file we get back has whatever case is in the manifest, so without this it is
// impossible to map back to the original path we gave to ExactMatcher.
struct RepoPathMap<V> {
    case_sensitive: bool,
    map: HashMap<RepoPathBuf, V>,
}

impl<V> RepoPathMap<V> {
    pub fn new(case_sensitive: bool) -> Self {
        Self {
            case_sensitive,
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: RepoPathBuf, value: V) -> Option<V> {
        match self.case_sensitive {
            true => self.map.insert(key, value),
            false => self.map.insert(key.to_lower_case(), value),
        }
    }

    pub fn get(&self, key: &RepoPathBuf) -> Option<&V> {
        match self.case_sensitive {
            true => self.map.get(key),
            false => self.map.get(&key.to_lower_case()),
        }
    }

    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, RepoPathBuf, V> {
        self.map.keys()
    }
}

impl IntoIterator for FileChangeDetector {
    type Item = Result<ResolvedFileChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(mut self) -> Self::IntoIter {
        // First, get the keys for the paths from the current manifest.

        let matcher = ExactMatcher::new(self.lookups.keys(), self.vfs.case_sensitive());
        let keys = self
            .manifest
            .read()
            .files(matcher)
            .filter_map(|result| {
                let file = match result {
                    Ok(file) => {
                        if manifest_flags_mismatch(
                            &self.vfs,
                            file.meta.file_type.into(),
                            self.lookups.get(&file.path).unwrap(),
                        ) {
                            tracing::trace!(path=?file.path, "changed (mf flags mismatch disk)");
                            self.results
                                .push(Ok(ResolvedFileChangeResult::changed(file.path)));
                            return None;
                        }

                        file
                    }
                    Err(e) => {
                        self.results.push(Err(e));
                        return None;
                    }
                };
                Some(Key::new(file.path, file.meta.hgid))
            })
            .collect::<Vec<_>>();

        // Then fetch the contents of each file and check it against the filesystem.
        // TODO: if the underlying stores gain the ability to do hash-based comparisons,
        // switch this to use that (rather than pulling down the entire contents of each
        // file).
        let vfs = self.vfs.clone();
        let comparisons = async_runtime::block_on(async {
            self.store
                .read_file_contents(keys)
                .await
                .map(|result| {
                    let (expected, key) = match result {
                        Ok(x) => x,
                        Err(e) => return Err(e),
                    };
                    compare_repo_bytes_to_disk(&vfs, expected, key.path)
                })
                .collect::<Vec<_>>()
                .await
        });
        self.results.extend(comparisons);
        self.results.into_iter()
    }
}

/// ParallelDetector uses a fixed number of worker threads to parallelize file
/// metadata checks and file content checks. The initial metadata checks are
/// parallelized, then the paths needing content check are collected into a big
/// list, fetched from the store, then there's a final parallelized step to
/// compared repo contents to disk contents.
///
/// ParallelDetector theoretically supports multiple submitters and consumers of
/// results, but note that results will not start flowing until all clones of a
/// ParallelDetector have been dropped (typically via conversion
/// to iterator).
#[derive(Clone)]
pub struct ParallelDetector {
    vfs: VFS,
    manifest: Arc<RwLock<TreeManifest>>,
    store: ArcReadFileContents,
    result_send: ProgressSender<Result<ResolvedFileChangeResult>>,
    result_recv: Receiver<Result<ResolvedFileChangeResult>>,
    check_contents_recv: Receiver<(RepoPathBuf, Metadata)>,
    // Store as an option so we can explicitly drop to disconnect the check_contents channel.
    check_metadata_send: Option<Sender<metadata::File>>,
    worker_count: usize,

    progress: Arc<ProgressBar>,
}

struct ProgressSender<T> {
    send: Sender<T>,
    bar: Arc<ProgressBar>,
}

impl<T> ProgressSender<T> {
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        self.bar.increase_position(1);
        self.send.send(msg)
    }
}

impl<T> Clone for ProgressSender<T> {
    fn clone(&self) -> Self {
        Self {
            send: self.send.clone(),
            bar: self.bar.clone(),
        }
    }
}

// Regarding error handling, all errors should be propagated to the user via the
// result_send channel. The exception is that any channel send errors are
// instead silently propagated, causing threads to exit. If we fail to send, that
// means the receiver has been dropped, so it is appropriate to cancel
// everything.
impl ParallelDetector {
    pub fn new(
        vfs: VFS,
        last_write: HgModifiedTime,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
        worker_count: usize,
    ) -> Self {
        // Channel to submit request for file's metadata to be checked against
        // treestate state. If the metadata check isn't conclusive, the path will be
        // forwarded for a full content check.
        let (check_metadata_send, check_metadata_recv) =
            crossbeam::channel::unbounded::<metadata::File>();

        // Channel to submit request to compare file's on-disk contents to repo's contents.
        let (check_contents_send, check_contents_recv) =
            crossbeam::channel::unbounded::<(RepoPathBuf, Metadata)>();

        // Channel for the detector to relay results back to the caller.
        let (result_send, result_recv) =
            crossbeam::channel::unbounded::<Result<ResolvedFileChangeResult>>();

        let bar = ProgressBar::register_new("comparing", 0, "files");

        let result_send = ProgressSender {
            send: result_send,
            bar: bar.clone(),
        };

        // Spin up workers to handle file metadata checks. The user submits
        // these via submit(). These threads will naturally exit once
        // check_metadata_send is dropped (thus disconnecting the channel).
        for _ in 0..worker_count {
            let check_metadata_recv = check_metadata_recv.clone();
            let vfs = vfs.clone();
            let check_contents_send = check_contents_send.clone();
            let result_send = result_send.clone();
            std::thread::spawn(move || -> Result<()> {
                for file in check_metadata_recv {
                    Self::perform_metadata_check(
                        &vfs,
                        file,
                        last_write,
                        &result_send,
                        &check_contents_send,
                    )?;
                }

                Ok(())
            });
        }

        Self {
            vfs,
            manifest,
            store,
            check_metadata_send: Some(check_metadata_send),
            result_send,
            result_recv,
            check_contents_recv,
            worker_count,
            progress: bar,
        }
    }

    // Read file bytes from disk and compare to the file's pristine repo bytes.
    fn compare_repo_bytes_to_disk(&self, repo_bytes: Bytes, path: RepoPathBuf) -> Result<()> {
        Ok(self
            .result_send
            .send(compare_repo_bytes_to_disk(&self.vfs, repo_bytes, path))?)
    }

    // Fetch the repo contents for all files needing content checks and then
    // submit work to compare each file's repo contents with the on-disk
    // contents.
    fn fetch_repo_contents(&self, disk_send: Sender<(RepoPathBuf, Bytes)>) -> Result<()> {
        // Slurp up all the paths needing content checks. The ReadFileContents
        // trait is already batched, so let's keep things simple and just build
        // one big batch. The alternative is to perform our own batching so we
        // can start comparing content before finishing comparing metadata, but
        // that probably isn't worth the trouble.
        let mut lookups: RepoPathMap<Metadata> = RepoPathMap::new(self.vfs.case_sensitive());
        for (path, metadata) in self.check_contents_recv.clone() {
            lookups.insert(path, metadata);
        }

        let matcher = ExactMatcher::new(lookups.keys(), self.vfs.case_sensitive());
        let keys = self
            .manifest
            .read()
            .files(matcher)
            .filter_map(|result| match result {
                Ok(file) => {
                    if manifest_flags_mismatch(
                        &self.vfs,
                        file.meta.file_type.into(),
                        lookups.get(&file.path).unwrap(),
                    ) {
                        tracing::trace!(path=?file.path, "changed (mf flags mismatch disk)");
                        match self
                            .result_send
                            .send(Ok(ResolvedFileChangeResult::changed(file.path)))
                        {
                            Ok(()) => None,
                            Err(err) => Some(Err(anyhow!(err))),
                        }
                    } else {
                        Some(Ok(Key::new(file.path, file.meta.hgid)))
                    }
                }
                Err(e) => match self.result_send.send(Err(e)) {
                    Ok(()) => None,
                    Err(err) => Some(Err(anyhow!(err))),
                },
            })
            .collect::<Result<Vec<_>>>()?;

        async_runtime::block_on(async {
            // TODO: if the underlying stores gain the ability to do hash-based comparisons,
            // switch this to use that (rather than pulling down the entire contents of each
            // file).
            let mut contents = self.store.read_file_contents(keys).await;

            while let Some(result) = contents.next().await {
                match result {
                    Ok((bytes, key)) => {
                        disk_send.send((key.path, bytes))?;
                    }
                    Err(e) => {
                        self.result_send.send(Err(e))?;
                    }
                };
            }

            Ok::<(), Error>(())
        })
    }

    // Perform comparison between a file's treestate state and on-disk metadata
    // (i.e. file size and modified time). If the comparison isn't conclusive,
    // submit a work unit for a full content check.
    fn perform_metadata_check(
        vfs: &VFS,
        mut file: metadata::File,
        last_write: HgModifiedTime,
        result_send: &ProgressSender<Result<ResolvedFileChangeResult>>,
        lookup_send: &Sender<(RepoPathBuf, Metadata)>,
    ) -> Result<()> {
        if file.fs_meta.is_none() {
            file.fs_meta = match vfs.metadata(&file.path) {
                Ok(metadata) => Some(metadata.into()),
                Err(e) => match e.downcast_ref::<std::io::Error>() {
                    Some(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    _ => {
                        result_send.send(Err(e))?;
                        return Ok(());
                    }
                },
            };
        }

        match file_changed_given_metadata(vfs, file, last_write) {
            Ok(res) => match res {
                FileChangeResult::Yes(change) => {
                    result_send.send(Ok(ResolvedFileChangeResult::Yes(change)))?;
                }
                FileChangeResult::No(path) => {
                    result_send.send(Ok(ResolvedFileChangeResult::No(path)))?;
                }
                FileChangeResult::Maybe((path, meta)) => {
                    lookup_send.send((path, meta))?;
                }
            },
            Err(err) => result_send.send(Err(err))?,
        }

        Ok(())
    }
}

impl FileChangeDetectorTrait for ParallelDetector {
    fn submit(&mut self, file: metadata::File) {
        self.check_metadata_send
            .as_ref()
            .unwrap()
            .send(file)
            .unwrap();
    }

    fn total_work_hint(&self, hint: u64) {
        self.progress.set_total(hint);
    }
}

impl IntoIterator for ParallelDetector {
    type Item = Result<ResolvedFileChangeResult>;
    type IntoIter = crossbeam::channel::IntoIter<Self::Item>;

    fn into_iter(mut self) -> Self::IntoIter {
        // Drop the metadata channel. This is important since it disconnects the
        // channel, causing the recv worker threads to exit. We do this so we
        // can continue to use self as a convenience container object below.
        self.check_metadata_send.take();

        let result_iter = self.result_recv.clone().into_iter();

        std::thread::spawn(move || -> Result<()> {
            let (disk_send, disk_recv) = crossbeam::channel::unbounded::<(RepoPathBuf, Bytes)>();

            // Spin up worker threads to read file contents from disk and
            // compare to repo contents. Threads will naturally exit when
            // disk_send is dropped (i.e. repo contents are done being fetched).
            for _ in 0..self.worker_count {
                let detector = self.clone();
                let disk_recv = disk_recv.clone();
                std::thread::spawn(move || -> Result<()> {
                    for (path, repo_bytes) in disk_recv {
                        detector.compare_repo_bytes_to_disk(repo_bytes, path)?;
                    }
                    Ok(())
                });
            }

            self.fetch_repo_contents(disk_send)
        });

        result_iter
    }
}
