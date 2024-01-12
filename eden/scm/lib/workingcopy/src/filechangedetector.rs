/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use pathmatcher::ExactMatcher;
use progress_model::ActiveProgressBar;
use progress_model::ProgressBar;
use storemodel::minibytes::Bytes;
use storemodel::FileStore;
use treestate::filestate::StateFlags;
use types::Key;
use types::RepoPathBuf;
use vfs::VFS;

use crate::filesystem::PendingChange;
use crate::metadata;
use crate::metadata::Metadata;

pub type ArcFileStore = Arc<dyn FileStore>;

pub(crate) enum FileChangeResult {
    Yes(PendingChange),
    No(RepoPathBuf),
    Maybe((RepoPathBuf, Metadata)),
}

impl FileChangeResult {
    fn changed(path: RepoPathBuf) -> Self {
        Self::Yes(PendingChange::Changed(path))
    }

    fn deleted(path: RepoPathBuf) -> Self {
        Self::Yes(PendingChange::Deleted(path))
    }
}

#[derive(Debug)]
pub(crate) enum ResolvedFileChangeResult {
    Yes(PendingChange),
    No((RepoPathBuf, Option<Metadata>)),
}

impl ResolvedFileChangeResult {
    fn changed(path: RepoPathBuf) -> Self {
        Self::Yes(PendingChange::Changed(path))
    }
}

pub(crate) trait FileChangeDetectorTrait:
    IntoIterator<Item = Result<ResolvedFileChangeResult>>
{
    fn submit(&mut self, file: metadata::File);
    fn total_work_hint(&self, _hint: u64) {}
}

pub(crate) struct FileChangeDetector {
    vfs: VFS,
    results: Vec<Result<ResolvedFileChangeResult>>,
    lookups: RepoPathMap<Metadata>,
    manifest: Arc<TreeManifest>,
    store: ArcFileStore,
    worker_count: usize,
    progress: ActiveProgressBar,
}

impl FileChangeDetector {
    pub fn new(
        vfs: VFS,
        manifest: Arc<TreeManifest>,
        store: ArcFileStore,
        worker_count: Option<usize>,
    ) -> Self {
        let case_sensitive = vfs.case_sensitive();
        FileChangeDetector {
            vfs,
            lookups: RepoPathMap::new(case_sensitive),
            results: Vec::new(),
            manifest,
            store,
            worker_count: worker_count.unwrap_or(10),
            progress: ProgressBar::new_adhoc("comparing", 0, "files"),
        }
    }
}

const NEED_CHECK: StateFlags = StateFlags::NEED_CHECK;
const EXIST_P1: StateFlags = StateFlags::EXIST_P1;

pub(crate) fn file_changed_given_metadata(
    vfs: &VFS,
    file: metadata::File,
) -> Result<FileChangeResult> {
    let path = file.path;

    let fs_meta = match file.fs_meta {
        Some(fs_meta) => fs_meta,
        None => match vfs.metadata(&path) {
            Ok(metadata) => Some(metadata.into()),
            Err(e) => match e.downcast_ref::<std::io::Error>() {
                Some(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                _ => return Err(e),
            },
        },
    };

    // First handle when metadata is None (i.e. file doesn't exist).
    let (fs_meta, state) = match (fs_meta, file.ts_state) {
        // File was untracked during crawl but no longer exists.
        (None, None) => {
            tracing::trace!(?path, "neither on disk nor in treestate");
            return Ok(FileChangeResult::No(path));
        }

        // File was not found but exists in P1: mark as deleted.
        (None, Some(state)) if state.state.intersects(EXIST_P1) => {
            tracing::trace!(?path, "not on disk, in P1");
            return Ok(FileChangeResult::deleted(path));
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
            return Ok(FileChangeResult::changed(path));
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

    if Some(ts_mtime) != fs_meta.mtime() {
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
    match vfs.read_with_metadata(&path) {
        Ok((disk_bytes, metadata)) => {
            if disk_bytes == repo_bytes {
                tracing::trace!(?path, "no (contents match)");
                Ok(ResolvedFileChangeResult::No((path, Some(metadata.into()))))
            } else {
                tracing::trace!(?path, "changed (contents mismatch)");
                Ok(ResolvedFileChangeResult::Yes(PendingChange::Changed(path)))
            }
        }
        Err(e) => {
            if let Some(e) = e.downcast_ref::<std::io::Error>() {
                if e.kind() == std::io::ErrorKind::NotFound {
                    tracing::trace!(?path, "deleted (file missing)");
                    return Ok(ResolvedFileChangeResult::Yes(PendingChange::Deleted(path)));
                }
            }

            if let Some(vfs::AuditError::ThroughSymlink(_)) = e.downcast_ref::<vfs::AuditError>() {
                tracing::trace!(?path, "deleted (read through symlink)");
                return Ok(ResolvedFileChangeResult::Yes(PendingChange::Deleted(path)));
            }

            tracing::trace!(?path, ?e);

            Err(e)
        }
    }
}

impl FileChangeDetector {
    pub(crate) fn has_changed_with_fresh_metadata(
        &mut self,
        file: metadata::File,
    ) -> Result<FileChangeResult> {
        let res = file_changed_given_metadata(&self.vfs, file);

        if let Ok(FileChangeResult::Maybe((ref path, ref meta))) = res {
            self.lookups.insert(path.to_owned(), meta.clone());
        }

        res
    }
}

impl FileChangeDetectorTrait for FileChangeDetector {
    fn submit(&mut self, file: metadata::File) {
        match self.has_changed_with_fresh_metadata(file) {
            Ok(res) => match res {
                FileChangeResult::Yes(change) => {
                    self.progress.increase_position(1);
                    self.results.push(Ok(ResolvedFileChangeResult::Yes(change)))
                }
                FileChangeResult::No(path) => {
                    self.progress.increase_position(1);
                    self.results
                        .push(Ok(ResolvedFileChangeResult::No((path, None))))
                }
                FileChangeResult::Maybe((path, meta)) => {
                    self.lookups.insert(path, meta);
                }
            },
            Err(err) => self.results.push(Err(err)),
        };
    }

    fn total_work_hint(&self, hint: u64) {
        self.progress.set_total(hint)
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

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

impl IntoIterator for FileChangeDetector {
    type Item = Result<ResolvedFileChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    #[tracing::instrument(skip_all)]
    fn into_iter(mut self) -> Self::IntoIter {
        let bar = self.progress;

        let _span = tracing::info_span!("check manifest", lookups = self.lookups.len()).entered();

        // First, get the keys for the paths from the current manifest.
        let matcher = ExactMatcher::new(self.lookups.keys(), self.vfs.case_sensitive());
        let keys = self
            .manifest
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
                            bar.increase_position(1);
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

        drop(_span);

        let _span = tracing::info_span!("compare contents", keys = keys.len()).entered();

        let (disk_send, disk_recv) = crossbeam::channel::unbounded::<(RepoPathBuf, Bytes)>();
        let (results_send, results_recv) =
            crossbeam::channel::unbounded::<Result<ResolvedFileChangeResult>>();

        for _ in 0..self.worker_count {
            let vfs = self.vfs.clone();
            let disk_recv = disk_recv.clone();
            let results_send = results_send.clone();
            let bar = bar.clone();
            std::thread::spawn(move || {
                for (path, repo_bytes) in disk_recv {
                    results_send
                        .send(compare_repo_bytes_to_disk(&vfs, repo_bytes, path))
                        .unwrap();
                    bar.increase_position(1);
                }
            });
        }

        // Then fetch the contents of each file and check it against the filesystem.
        // TODO: if the underlying stores gain the ability to do hash-based comparisons,
        // switch this to use that (rather than pulling down the entire contents of each
        // file).
        let _span = tracing::info_span!("get_content_stream").entered();
        match self.store.get_content_iter(keys) {
            Err(e) => results_send.send(Err(e)).unwrap(),
            Ok(v) => {
                for entry in v {
                    let (key, data) = match entry {
                        Ok(v) => v,
                        Err(e) => {
                            results_send.send(Err(e)).unwrap();
                            continue;
                        }
                    };
                    disk_send.send((key.path, data)).unwrap();
                }
            }
        };

        drop(results_send);
        drop(disk_send);

        self.results.extend(results_recv.into_iter());
        self.results.into_iter()
    }
}
