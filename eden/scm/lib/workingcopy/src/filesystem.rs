/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fs::Metadata;
use std::path::PathBuf;
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
use pathmatcher::Matcher;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::is_executable;
use vfs::is_symlink;
use vfs::VFS;

use crate::walker::WalkEntry;
use crate::walker::WalkError;
use crate::walker::Walker;

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

pub struct PhysicalFileSystem {
    // TODO: Make this an Arc<Mutex<VFS>> so we can persist the vfs pathauditor cache
    vfs: VFS,
}

impl PhysicalFileSystem {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(PhysicalFileSystem {
            vfs: VFS::new(root)?,
        })
    }

    pub fn pending_changes<M: Matcher + Clone + Send + Sync + 'static>(
        &self,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
        treestate: Arc<Mutex<TreeState>>,
        matcher: M,
        include_directories: bool,
        last_write: HgModifiedTime,
        num_threads: u8,
    ) -> Result<PendingChanges<M>> {
        let walker = Walker::new(
            self.vfs.root().to_path_buf(),
            matcher.clone(),
            false,
            num_threads,
        )?;
        let pending_changes = PendingChanges {
            vfs: self.vfs.clone(),
            walker,
            matcher,
            manifest,
            store,
            treestate,
            stage: PendingChangesStage::Walk,
            include_directories,
            seen: HashSet::new(),
            lookups: vec![],
            tree_iter: None,
            lookup_iter: None,
            last_write,
        };
        Ok(pending_changes)
    }
}

pub struct PendingChanges<M: Matcher + Clone + Send + Sync + 'static> {
    vfs: VFS,
    walker: Walker<M>,
    matcher: M,
    manifest: Arc<RwLock<TreeManifest>>,
    store: ArcReadFileContents,
    treestate: Arc<Mutex<TreeState>>,
    stage: PendingChangesStage,
    include_directories: bool,
    seen: HashSet<RepoPathBuf>,
    lookups: Vec<RepoPathBuf>,
    tree_iter: Option<Box<dyn Iterator<Item = Result<PendingChangeResult>> + Send>>,
    lookup_iter: Option<Box<dyn Iterator<Item = Result<PendingChangeResult>> + Send>>,
    last_write: HgModifiedTime,
}

#[derive(PartialEq)]
enum PendingChangesStage {
    Walk,
    IterateTree,
    Lookups,
    Finished,
}

impl PendingChangesStage {
    pub fn next(&self) -> PendingChangesStage {
        match self {
            PendingChangesStage::Walk => PendingChangesStage::IterateTree,
            PendingChangesStage::IterateTree => PendingChangesStage::Lookups,
            PendingChangesStage::Lookups => PendingChangesStage::Finished,
            PendingChangesStage::Finished => PendingChangesStage::Finished,
        }
    }
}

pub enum ChangeType {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
}

pub enum PendingChangeResult {
    File(ChangeType),
    SeenDirectory(RepoPathBuf),
}

impl<M: Matcher + Clone + Send + Sync + 'static> PendingChanges<M> {
    fn is_changed(&mut self, path: &RepoPath, metadata: &Metadata) -> Result<bool> {
        let mut treestate = self.treestate.lock();
        let state = treestate.get(path)?;

        let state = match state {
            Some(state) => state,
            // File exists but is not in the treestate (untracked)
            None => return Ok(true),
        };

        // If it's not in P1, (i.e. it's added or untracked) it's considered changed.
        let flags = state.state;
        let in_parent = flags.intersects(StateFlags::EXIST_P1); // TODO: Also check against P2?
        if !in_parent {
            return Ok(true);
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
                return Ok(true);
            }
        }

        // If it's marked NEED_CHECK, we always need to do a lookup, regardless of the mtime.
        let needs_check = flags.intersects(StateFlags::NEED_CHECK) || !valid_size;
        if needs_check {
            self.lookups.push(path.to_owned());
            return Ok(false);
        }

        // If the mtime has changed or matches the last normal() write time, we need to compare the
        // file contents in the later Lookups phase.  mtime can be negative as well. A -1 indicates
        // the file is in a lookup state. Since a -1 will always cause the equality comparison
        // below to fail and force a lookup, the -1 is handled correctly without special casing. In
        // theory all -1 files should be marked NEED_CHECK above (I think).
        if state.mtime < 0 {
            self.lookups.push(path.to_owned());
        } else {
            let state_mtime: Result<HgModifiedTime> = state.mtime.try_into();
            let state_mtime =
                state_mtime.map_err(|e| WalkError::InvalidMTime(path.to_owned(), e))?;
            let mtime: HgModifiedTime = metadata.modified()?.try_into()?;

            if mtime != state_mtime || mtime == self.last_write {
                self.lookups.push(path.to_owned());
            }
        }

        Ok(false)
    }

    fn next_walk(&mut self) -> Option<Result<PendingChangeResult>> {
        loop {
            match self.walker.next() {
                Some(Ok(WalkEntry::File(file, metadata))) => {
                    let file = normalize(file);
                    self.seen.insert(file.to_owned());
                    let changed = match self.is_changed(&file, &metadata) {
                        Ok(result) => result,
                        Err(e) => return Some(Err(e)),
                    };

                    if changed {
                        return Some(Ok(PendingChangeResult::File(ChangeType::Changed(file))));
                    }
                }
                Some(Ok(WalkEntry::Directory(dir))) => {
                    if self.include_directories {
                        let dir = normalize(dir);
                        return Some(Ok(PendingChangeResult::SeenDirectory(dir)));
                    }
                }
                Some(Err(e)) => {
                    return Some(Err(e));
                }
                None => {
                    return None;
                }
            };
        }
    }

    fn next_tree(&mut self) -> Option<Result<PendingChangeResult>> {
        if self.tree_iter.is_none() {
            self.tree_iter = Some(Box::new(self.get_tree_entries().into_iter()));
        }

        self.tree_iter.as_mut().unwrap().next()
    }

    fn get_tree_entries(&mut self) -> Vec<Result<PendingChangeResult>> {
        let mut results = vec![];
        let tracked = self.get_tracked_from_p1();
        if let Err(e) = tracked {
            results.push(Err(e));
            return results;
        }
        let tracked = tracked.unwrap();

        for path in tracked.into_iter() {
            // Skip this path if we've seen it or it doesn't match the matcher.
            if self.seen.contains(&path) {
                continue;
            } else {
                match self.matcher.matches_file(&path) {
                    Err(e) => {
                        results.push(Err(e));
                        continue;
                    }
                    Ok(false) => continue,
                    Ok(true) => {}
                }
            }

            // If it's behind a symlink consider it deleted.
            let metadata = self.vfs.metadata(&path);

            // TODO: audit the path for symlinks and weirdness
            // If it's missing or not readable, consider it deleted.
            let metadata = match metadata {
                Ok(metadata) => metadata,
                Err(_) => {
                    results.push(Ok(PendingChangeResult::File(ChangeType::Deleted(path))));
                    continue;
                }
            };

            let file_type = metadata.file_type();

            // If the file is not a normal file or a symlink (ex: it could be a directory or a
            // weird file like a fifo file), consider it deleted.
            if !file_type.is_file() || file_type.is_symlink() {
                results.push(Ok(PendingChangeResult::File(ChangeType::Deleted(path))));
                continue;
            }

            // In an ideal world we wouldn't see any paths that exist on disk that weren't found by
            // the walk phase, but there can be ignored files that the walk ignores but that are in
            // the dirstate. So we compare them here to see if they changed.
            let changed = match self.is_changed(&path, &metadata) {
                Ok(result) => result,
                Err(e) => {
                    results.push(Err(e));
                    continue;
                }
            };

            if changed {
                results.push(Ok(PendingChangeResult::File(ChangeType::Changed(path))));
            }
        }
        results
    }

    /// Returns the files in the treestate that are from p1.
    /// We only care about files from p1 because pending_changes is relative to p1.
    fn get_tracked_from_p1(&mut self) -> Result<Vec<RepoPathBuf>> {
        let mut treestate = self.treestate.lock();

        let mut result = Vec::new();
        let mask = StateFlags::EXIST_P1;

        treestate.visit(
            &mut |components, _| {
                let path = components.concat();
                let path = RepoPathBuf::from_utf8(path)?;
                result.push(path);
                Ok(VisitorResult::NotChanged)
            },
            &|_path, dir| match dir.get_aggregated_state() {
                None => true,
                Some(state) => state.union.intersects(mask),
            },
            &|_path, file| file.state.intersects(mask),
        )?;
        Ok(result)
    }

    fn next_lookup(&mut self) -> Option<Result<PendingChangeResult>> {
        self.lookup_iter
            .get_or_insert_with(|| {
                // The first time this function is called, process all of the pending lookups.
                let mut results = Vec::<Result<PendingChangeResult>>::new();

                // First, get the keys for the paths from the current manifest.
                let matcher = match ExactMatcher::new(self.lookups.iter()) {
                    Ok(matcher) => matcher,
                    Err(e) => return Box::new(std::iter::once(Err(e))),
                };
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
                                Some(Ok(PendingChangeResult::File(ChangeType::Changed(key.path))))
                            }
                        })
                        .collect::<Vec<_>>()
                        .await
                });
                results.extend(comparisons);
                Box::new(results.into_iter())
            })
            .next()
    }

    // TODO: after finishing these comparisons, update the cached mtimes of files so we
    // don't have to do a comparison again next time.
}

impl<M: Matcher + Clone + Send + Sync + 'static> Iterator for PendingChanges<M> {
    type Item = Result<PendingChangeResult>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Try to make this into a chain instead of a manual state machine
        loop {
            let change = match self.stage {
                PendingChangesStage::Walk => self.next_walk(),
                PendingChangesStage::IterateTree => self.next_tree(),
                PendingChangesStage::Lookups => self.next_lookup(),
                PendingChangesStage::Finished => None,
            };

            if change.is_some() {
                return change;
            }

            self.stage = self.stage.next();
            if self.stage == PendingChangesStage::Finished {
                return None;
            }
        }
    }
}

fn normalize(path: RepoPathBuf) -> RepoPathBuf {
    // TODO: Support path normalization on case insensitive file systems
    path
}
