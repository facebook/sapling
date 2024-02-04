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
use configmodel::Config;
use configmodel::ConfigExt;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use repolock::RepoLocker;
use storemodel::FileStore;
use termlogger::TermLogger;
use treestate::filestate::StateFlags;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::RepoPathBuf;
use vfs::VFS;

use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::FileChangeResult;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;
use crate::metadata;
use crate::metadata::Metadata;
use crate::util::dirstate_write_time_override;
use crate::util::maybe_flush_treestate;
use crate::util::update_filestate_from_fs_meta;
use crate::walker::WalkEntry;
use crate::walker::Walker;
use crate::workingcopy::WorkingCopy;

type ArcFileStore = Arc<dyn FileStore>;
type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct PhysicalFileSystem {
    // TODO: Make this an Arc<Mutex<VFS>> so we can persist the vfs pathauditor cache
    pub(crate) vfs: VFS,
    pub(crate) tree_resolver: ArcReadTreeManifest,
    pub(crate) store: ArcFileStore,
    pub(crate) treestate: Arc<Mutex<TreeState>>,
    pub(crate) locker: Arc<RepoLocker>,
    pub(crate) dot_dir: String,
}

impl PhysicalFileSystem {
    pub fn new(
        vfs: VFS,
        tree_resolver: ArcReadTreeManifest,
        store: ArcFileStore,
        treestate: Arc<Mutex<TreeState>>,
        locker: Arc<RepoLocker>,
    ) -> Result<Self> {
        let ident = identity::must_sniff_dir(vfs.root())?;
        Ok(PhysicalFileSystem {
            vfs,
            tree_resolver,
            store,
            treestate,
            locker,
            dot_dir: ident.dot_dir().to_string(),
        })
    }
}

impl FileSystem for PhysicalFileSystem {
    fn pending_changes(
        &self,
        matcher: DynMatcher,
        ignore_matcher: DynMatcher,
        ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
        config: &dyn Config,
        _lgr: &TermLogger,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let walker = Walker::new(
            self.vfs.root().to_path_buf(),
            self.dot_dir.clone(),
            ignore_dirs,
            matcher.clone(),
            false,
        )?;
        let manifests =
            WorkingCopy::current_manifests(&self.treestate.lock(), &self.tree_resolver)?;
        let file_change_detector = FileChangeDetector::new(
            self.vfs.clone(),
            manifests[0].clone(),
            self.store.clone(),
            config.get_opt("workingcopy", "worker-count")?,
        );
        let pending_changes = PendingChanges {
            walker,
            matcher,
            ignore_matcher,
            include_ignored,
            treestate: self.treestate.clone(),
            stage: PendingChangesStage::Walk,
            seen: HashSet::new(),
            tree_iter: None,
            lookup_iter: None,
            file_change_detector: Some(file_change_detector),
            update_ts: Vec::new(),
            locker: self.locker.clone(),
            dirstate_write_time: dirstate_write_time_override(config),
            vfs: self.vfs.clone(),
        };
        Ok(Box::new(pending_changes))
    }

    fn sparse_matcher(
        &self,
        manifests: &[Arc<TreeManifest>],
        dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        crate::sparse::sparse_matcher(
            &self.vfs,
            manifests,
            self.store.clone(),
            &self.vfs.root().join(dot_dir),
        )
    }
}

pub struct PendingChanges<M: Matcher + Clone + Send + Sync + 'static> {
    walker: Walker<M>,
    matcher: M,
    ignore_matcher: M,
    include_ignored: bool,
    treestate: Arc<Mutex<TreeState>>,
    stage: PendingChangesStage,
    seen: HashSet<RepoPathBuf>,
    tree_iter: Option<Box<dyn Iterator<Item = Result<PendingChange>> + Send>>,
    lookup_iter: Option<Box<dyn Iterator<Item = Result<ResolvedFileChangeResult>> + Send>>,
    file_change_detector: Option<FileChangeDetector>,
    update_ts: Vec<(RepoPathBuf, Metadata)>,
    locker: Arc<RepoLocker>,
    dirstate_write_time: Option<i64>,
    vfs: VFS,
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
    fn next_walk(&mut self) -> Result<Option<PendingChange>> {
        loop {
            match self.walker.next() {
                Some(Ok(WalkEntry::File(mut path, metadata))) => {
                    tracing::trace!(%path, "found file");

                    if self.include_ignored && self.ignore_matcher.matches_file(&path)? {
                        tracing::trace!(%path, "ignored");
                        return Ok(Some(PendingChange::Ignored(path)));
                    }

                    let mut ts = self.treestate.lock();

                    // On case insensitive systems, normalize the path so
                    // duplicate paths with different case can be detected in
                    // the seen set, but only if the dirstate entry hasn't been
                    // deleted.
                    let (normalized, mut ts_state) = ts.normalize_path_and_get(path.as_ref())?;
                    if normalized != path.as_byte_slice() {
                        let normalized = RepoPathBuf::from_utf8(normalized.into_owned())?;

                        match &ts_state {
                            None => {
                                // File is not currently tracked. The normalized path can
                                // diff if the file's path has a directory prefix that
                                // matches case insensitively with something else in the
                                // treestate. In that case, we should use the case from
                                // the treestate to avoid unnecessary directory case
                                // divergence in the treestate.
                                tracing::trace!(%path, %normalized, "normalizing untracked file");
                                path = normalized;
                            }
                            Some(s) if s.state.intersects(StateFlags::EXIST_NEXT) => {
                                tracing::trace!(%path, %normalized, "normalizing path based on dirstate");
                                path = normalized;
                            }
                            Some(_) => {
                                tracing::trace!(%path, %normalized, "not normalizing because !EXIST_NEXT");
                                // We are staying separate from the normalized entry, so we mustn't use
                                // the normalized path's entry.
                                ts_state = ts.get(&path)?.cloned();
                            }
                        }
                    }
                    self.seen.insert(path.clone());
                    let changed = self
                        .file_change_detector
                        .as_mut()
                        .unwrap()
                        .has_changed_with_fresh_metadata(metadata::File {
                            path,
                            ts_state,
                            fs_meta: Some(Some(metadata.into())),
                        })?;

                    if let FileChangeResult::Yes(change_type) = changed {
                        return Ok(Some(change_type));
                    }
                }
                Some(Ok(WalkEntry::Directory(_))) => {
                    // Shouldn't happen since we don't request directories.
                }
                Some(Err(e)) => {
                    return Err(e);
                }
                None => {
                    return Ok(None);
                }
            };
        }
    }

    fn next_tree(&mut self) -> Option<Result<PendingChange>> {
        if self.tree_iter.is_none() {
            self.tree_iter = Some(Box::new(self.get_tree_entries().into_iter()));
        }

        self.tree_iter.as_mut().unwrap().next()
    }

    fn get_tree_entries(&mut self) -> Vec<Result<PendingChange>> {
        let tracked = match self.get_tracked_from_p1() {
            Err(e) => return vec![Err(e)],
            Ok(tracked) => tracked,
        };
        let mut ts = self.treestate.lock();

        tracked
            .into_iter()
            .filter_map(|mut path| {
                tracing::trace!(%path, "tree path");

                let normalized = match ts.normalize_path(path.as_ref()) {
                    Ok(path) => path,
                    Err(e) => return Some(Err(e)),
                };
                if normalized != path.as_byte_slice() {
                    let normalized = match RepoPathBuf::from_utf8(normalized.into_owned()) {
                        Ok(path) => path,
                        Err(e) => return Some(Err(e.into())),
                    };
                    tracing::trace!(%path, %normalized, "normalized tree path");
                    path = normalized;
                }

                // Skip this path if we've seen it or it doesn't match the matcher.
                if self.seen.contains(&path) {
                    tracing::trace!(%path, "tree path seen");
                    None
                } else {
                    match self.matcher.matches_file(&path) {
                        Err(e) => Some(Err(e)),
                        Ok(false) => {
                            tracing::trace!(%path, "tree path doesn't match");
                            None
                        }
                        // This path is EXIST_P1 but not on disk - emit deleted event.
                        Ok(true) => {
                            tracing::trace!(%path, "tree path deleted");
                            Some(Ok(PendingChange::Deleted(path.to_owned())))
                        }
                    }
                }
            })
            .collect()
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

    fn next_lookup(&mut self) -> Option<Result<PendingChange>> {
        loop {
            let next = self
                .lookup_iter
                .get_or_insert_with(|| {
                    Box::new(self.file_change_detector.take().unwrap().into_iter())
                })
                .next()?;

            match next {
                Ok(ResolvedFileChangeResult::Yes(change_type)) => {
                    return Some(Ok(change_type));
                }
                Ok(ResolvedFileChangeResult::No((path, fs_meta))) => {
                    if let Some(fs_meta) = fs_meta {
                        self.update_ts.push((path, fs_meta));
                    }
                    continue;
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

impl<M: Matcher + Clone + Send + Sync + 'static> Iterator for PendingChanges<M> {
    type Item = Result<PendingChange>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Try to make this into a chain instead of a manual state machine
        loop {
            let change = match self.stage {
                PendingChangesStage::Walk => self.next_walk().transpose(),
                PendingChangesStage::IterateTree => self.next_tree(),
                PendingChangesStage::Lookups => self.next_lookup(),
                PendingChangesStage::Finished => None,
            };

            if change.is_some() {
                return change;
            }

            self.stage = self.stage.next();
            if self.stage == PendingChangesStage::Finished {
                if let Err(err) = self.update_treestate_mtimes() {
                    return Some(Err(err));
                }

                return None;
            }
        }
    }
}

impl<M: Matcher + Clone + Send + Sync + 'static> PendingChanges<M> {
    fn update_treestate_mtimes(&mut self) -> Result<()> {
        let mut ts = self.treestate.lock();
        let was_dirty = ts.dirty();

        // If file came back clean, update dirstate entry with current mtime and/or size.
        for (path, fs_meta) in self.update_ts.drain(..) {
            if let Some(state) = ts.get(&path)? {
                tracing::trace!(%path, "updating treestate metadata");

                let mut state = state.clone();

                // We don't set NEED_CHECK since we check all files every time.
                // However, unset it anyway in case someone else set it
                // (otherwise files get stuck NEED_CHECK).
                state.state -= StateFlags::NEED_CHECK;

                update_filestate_from_fs_meta(&mut state, &fs_meta);
                ts.insert(&path, &state)?;
            }
        }

        // Don't flush treestate if it was already dirty. If we are inside a
        // Python transaction with uncommitted, substantial dirstate changes,
        // those changes should not be written out until the transaction
        // finishes.
        if !was_dirty {
            maybe_flush_treestate(
                self.vfs.root(),
                &mut ts,
                &self.locker,
                self.dirstate_write_time.clone(),
            )?;
        }

        Ok(())
    }
}
