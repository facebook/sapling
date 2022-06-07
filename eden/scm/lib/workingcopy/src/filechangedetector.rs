/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::Metadata;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Error;
use anyhow::Result;
use futures::StreamExt;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use parking_lot::RwLock;
use pathmatcher::ExactMatcher;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::is_executable;
use vfs::is_symlink;
use vfs::VFS;

use crate::walker::WalkError;

type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;

/// Represents a file modification time in Mercurial, in seconds since the unix epoch.
#[derive(PartialEq)]
pub struct HgModifiedTime(u64);

impl From<u64> for HgModifiedTime {
    fn from(value: u64) -> Self {
        HgModifiedTime(value)
    }
}

impl From<u32> for HgModifiedTime {
    fn from(value: u32) -> Self {
        HgModifiedTime(value.into())
    }
}

impl TryFrom<SystemTime> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: SystemTime) -> Result<Self> {
        Ok(value
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs()
            .into())
    }
}

impl TryFrom<i32> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(HgModifiedTime(value.try_into()?))
    }
}

pub enum FileChangeResult {
    Yes,
    No,
    Maybe,
}

pub trait FileChangeDetectorTrait {
    fn has_changed(&mut self, path: &RepoPath, metadata: &Metadata) -> Result<FileChangeResult>;

    fn resolve_maybes(&self) -> Box<dyn Iterator<Item = Result<RepoPathBuf>> + Send>;
}

pub struct FileChangeDetector {
    treestate: Arc<Mutex<TreeState>>,
    vfs: VFS,
    last_write: HgModifiedTime,
    lookups: Vec<RepoPathBuf>,
    manifest: Arc<RwLock<TreeManifest>>,
    store: ArcReadFileContents,
}

impl FileChangeDetector {
    pub fn new(
        treestate: Arc<Mutex<TreeState>>,
        vfs: VFS,
        last_write: HgModifiedTime,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
    ) -> Self {
        let lookups: Vec<RepoPathBuf> = vec![];
        FileChangeDetector {
            treestate,
            vfs,
            last_write,
            lookups,
            manifest,
            store,
        }
    }
}

impl FileChangeDetectorTrait for FileChangeDetector {
    fn has_changed(&mut self, path: &RepoPath, metadata: &Metadata) -> Result<FileChangeResult> {
        let mut treestate = self.treestate.lock();
        let state = treestate.get(path)?;

        let state = match state {
            Some(state) => state,
            // File exists but is not in the treestate (untracked)
            None => return Ok(FileChangeResult::Yes),
        };

        // If it's not in P1, (i.e. it's added or untracked) it's considered changed.
        let flags = state.state;
        let in_parent = flags.intersects(StateFlags::EXIST_P1); // TODO: Also check against P2?
        if !in_parent {
            return Ok(FileChangeResult::Yes);
        }

        // If working copy file size or flags are different from what is in treestate, it has changed.
        // Note: state.size is i32 since Mercurial uses negative numbers to indicate special files.
        // A -1 indicates the file is either in a merge state or a lookup state.
        // A -2 indicates the file comes from the other parent (and may or may not exist in the
        // current parent).
        //
        // Regardless, if the size is negative, we'll do a lookup comparison since we can't
        // determine if the file has changed relative to p1. This logic is a mess and we should get
        // rid of all these negative numbers.
        let valid_size = state.size >= 0;
        if valid_size {
            let size_different = metadata.len() != state.size.try_into().unwrap_or(std::u64::MAX);
            let exec_different =
                self.vfs.supports_executables() && is_executable(metadata) != state.is_executable();
            let symlink_different =
                self.vfs.supports_symlinks() && is_symlink(metadata) != state.is_symlink();

            if size_different || exec_different || symlink_different {
                return Ok(FileChangeResult::Yes);
            }
        }

        // If it's marked NEED_CHECK, we always need to do a lookup, regardless of the mtime.
        let needs_check = flags.intersects(StateFlags::NEED_CHECK) || !valid_size;
        if needs_check {
            self.lookups.push(path.to_owned());
            return Ok(FileChangeResult::Maybe);
        }

        // If the mtime has changed or matches the last normal() write time, we need to compare the
        // file contents in the later Lookups phase.  mtime can be negative as well. A -1 indicates
        // the file is in a lookup state. Since a -1 will always cause the equality comparison
        // below to fail and force a lookup, the -1 is handled correctly without special casing. In
        // theory all -1 files should be marked NEED_CHECK above (I think).
        if state.mtime < 0 {
            self.lookups.push(path.to_owned());
            return Ok(FileChangeResult::Maybe);
        }

        let state_mtime: Result<HgModifiedTime> = state.mtime.try_into();
        let state_mtime = state_mtime.map_err(|e| WalkError::InvalidMTime(path.to_owned(), e))?;
        let mtime: HgModifiedTime = metadata.modified()?.try_into()?;

        if mtime != state_mtime || mtime == self.last_write {
            self.lookups.push(path.to_owned());
            return Ok(FileChangeResult::Maybe);
        }

        Ok(FileChangeResult::No)
    }

    fn resolve_maybes(&self) -> Box<dyn Iterator<Item = Result<RepoPathBuf>> + Send> {
        let mut results = Vec::<Result<RepoPathBuf>>::new();

        // First, get the keys for the paths from the current manifest.
        let matcher = ExactMatcher::new(self.lookups.iter());
        let keys = self
            .manifest
            .read()
            .files(matcher)
            .filter_map(|result| {
                let file = match result {
                    Ok(file) => file,
                    Err(e) => {
                        results.push(Err(e));
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
                .filter_map(|result| async {
                    let (expected, key) = match result {
                        Ok(x) => x,
                        Err(e) => return Some(Err(e)),
                    };
                    let actual = match vfs.read(&key.path) {
                        Ok(x) => x,
                        Err(e) => return Some(Err(e)),
                    };
                    if expected == actual {
                        None
                    } else {
                        Some(Ok(key.path))
                    }
                })
                .collect::<Vec<_>>()
                .await
        });
        results.extend(comparisons);
        Box::new(results.into_iter())
    }

    // TODO: after finishing these comparisons, update the cached mtimes of files so we
    // don't have to do a comparison again next time.
}
