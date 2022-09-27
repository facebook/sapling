/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use manifest_tree::ReadTreeManifest;
use parking_lot::Mutex;
use pathmatcher::Matcher;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::RepoPathBuf;
use vfs::VFS;

use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::FileChangeResult;
use crate::filechangedetector::HgModifiedTime;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges as PendingChangesTrait;
use crate::walker::WalkEntry;
use crate::walker::Walker;
use crate::workingcopy::WorkingCopy;

type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;
type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct PhysicalFileSystem {
    // TODO: Make this an Arc<Mutex<VFS>> so we can persist the vfs pathauditor cache
    vfs: VFS,
    tree_resolver: ArcReadTreeManifest,
    store: ArcReadFileContents,
    treestate: Arc<Mutex<TreeState>>,
    include_directories: bool,
    last_write: HgModifiedTime,
    num_threads: u8,
}

impl PhysicalFileSystem {
    pub fn new(
        root: PathBuf,
        tree_resolver: ArcReadTreeManifest,
        store: ArcReadFileContents,
        treestate: Arc<Mutex<TreeState>>,
        include_directories: bool,
        last_write: HgModifiedTime,
        num_threads: u8,
    ) -> Result<Self> {
        Ok(PhysicalFileSystem {
            vfs: VFS::new(root)?,
            tree_resolver,
            store,
            treestate,
            include_directories,
            last_write,
            num_threads,
        })
    }
}

impl PendingChangesTrait for PhysicalFileSystem {
    fn pending_changes(
        &self,
        matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let root = self.vfs.root().to_path_buf();
        let ident = identity::must_sniff_dir(&root)?;
        let walker = Walker::new(
            root,
            ident.dot_dir().to_string(),
            matcher.clone(),
            false,
            self.num_threads,
        )?;
        let manifests =
            WorkingCopy::current_manifests(&self.treestate.lock(), &self.tree_resolver)?;
        let file_change_detector = FileChangeDetector::new(
            self.treestate.clone(),
            self.vfs.clone(),
            self.last_write.clone(),
            manifests[0].clone(),
            self.store.clone(),
        );
        let pending_changes = PendingChanges {
            walker,
            matcher,
            treestate: self.treestate.clone(),
            stage: PendingChangesStage::Walk,
            include_directories: self.include_directories,
            seen: HashSet::new(),
            tree_iter: None,
            lookup_iter: None,
            file_change_detector,
        };
        Ok(Box::new(pending_changes))
    }
}

pub struct PendingChanges<M: Matcher + Clone + Send + Sync + 'static> {
    walker: Walker<M>,
    matcher: M,
    treestate: Arc<Mutex<TreeState>>,
    stage: PendingChangesStage,
    include_directories: bool,
    seen: HashSet<RepoPathBuf>,
    tree_iter: Option<Box<dyn Iterator<Item = Result<PendingChangeResult>> + Send>>,
    lookup_iter: Option<Box<dyn Iterator<Item = Result<PendingChangeResult>> + Send>>,
    file_change_detector: FileChangeDetector,
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

impl<M: Matcher + Clone + Send + Sync + 'static> PendingChanges<M> {
    fn next_walk(&mut self) -> Option<Result<PendingChangeResult>> {
        loop {
            match self.walker.next() {
                Some(Ok(WalkEntry::File(file, metadata))) => {
                    let file = normalize(file);
                    self.seen.insert(file.to_owned());
                    let changed = match self
                        .file_change_detector
                        .has_changed_with_fresh_metadata(&file, metadata)
                    {
                        Ok(result) => result,
                        Err(e) => return Some(Err(e)),
                    };

                    if let FileChangeResult::Yes(change_type) = changed {
                        return Some(Ok(PendingChangeResult::File(change_type)));
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

            let changed = match self.file_change_detector.has_changed(&path) {
                Ok(result) => result,
                Err(e) => {
                    results.push(Err(e));
                    continue;
                }
            };

            if let FileChangeResult::Yes(change_type) = changed {
                // We expect the change type to be deleted here because in an ideal world we
                // wouldn't see any paths that exist on disk that weren't found by the walk phase,
                // but there can be ignored files that the walk ignores but that are in the
                // dirstate. So we compare them here to see if they changed.
                results.push(Ok(PendingChangeResult::File(change_type)));
            }
        }
        results
    }

    /// Returns the files in the treestate that are from p1.
    /// We only care about files from p1 because pending_changes is relative to p1.
    fn get_tracked_from_p1(&self) -> Result<Vec<RepoPathBuf>> {
        let mut result = Vec::new();
        let mask = StateFlags::EXIST_P1;

        self.treestate.lock().visit(
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
                let iter = self
                    .file_change_detector
                    .resolve_maybes()
                    .filter_map(|result| match result {
                        Ok(ResolvedFileChangeResult::Yes(change_type)) => {
                            Some(Ok(PendingChangeResult::File(change_type)))
                        }
                        Ok(ResolvedFileChangeResult::No(_)) => None,
                        Err(e) => Some(Err(e)),
                    });
                Box::new(iter)
            })
            .next()
    }
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
