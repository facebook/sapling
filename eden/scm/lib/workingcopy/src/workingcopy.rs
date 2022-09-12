/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use manifest_tree::TreeManifest;
use parking_lot::RwLock;
use pathmatcher::Matcher;
use status::Status;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::RepoPathBuf;

#[cfg(feature = "eden")]
use crate::edenfs::EdenFileSystem;
use crate::filechangedetector::HgModifiedTime;
use crate::filesystem::FileSystemType;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;
use crate::physicalfs::PhysicalFileSystem;
use crate::status::compute_status;
use crate::watchmanfs::WatchmanFileSystem;

type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;
type FileSystem = Box<dyn PendingChanges>;

pub struct WorkingCopy {
    treestate: Rc<RefCell<TreeState>>,
    manifest: Arc<RwLock<TreeManifest>>,
    filesystem: FileSystem,
}

impl WorkingCopy {
    pub fn new(
        root: PathBuf,
        // TODO: Have constructor figure out FileSystemType
        file_system_type: FileSystemType,
        treestate: TreeState,
        manifest: TreeManifest,
        store: ArcReadFileContents,
        last_write: SystemTime,
    ) -> std::result::Result<Self, (TreeState, Error)> {
        let treestate = Rc::new(RefCell::new(treestate));
        let manifest = Arc::new(RwLock::new(manifest));

        let filesystem: Result<FileSystem> = Self::construct_file_system(
            root,
            file_system_type,
            treestate.clone(),
            manifest.clone(),
            store,
            last_write,
        );

        let filesystem = match filesystem {
            Ok(fs) => fs,
            Err(e) => {
                let treestate = Rc::try_unwrap(treestate)
                    .expect("No clones created yet")
                    .into_inner();
                return Err((treestate, e));
            }
        };

        Ok(WorkingCopy {
            treestate,
            manifest,
            filesystem,
        })
    }

    fn construct_file_system(
        root: PathBuf,
        file_system_type: FileSystemType,
        treestate: Rc<RefCell<TreeState>>,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
        last_write: SystemTime,
    ) -> Result<FileSystem> {
        let last_write: HgModifiedTime = last_write.try_into()?;

        Ok(match file_system_type {
            FileSystemType::Normal => Box::new(PhysicalFileSystem::new(
                root,
                manifest.clone(),
                store,
                treestate.clone(),
                false,
                last_write,
                8,
            )?),
            FileSystemType::Watchman => Box::new(WatchmanFileSystem::new(
                root,
                treestate.clone(),
                manifest.clone(),
                store,
                last_write,
            )?),
            FileSystemType::Eden => {
                #[cfg(not(feature = "eden"))]
                panic!("cannot use EdenFS in a non-EdenFS build");
                #[cfg(feature = "eden")]
                Box::new(EdenFileSystem::new(root)?)
            }
        })
    }

    // TODO: Remove this method once the pyworkingcopy status bindings have been
    // deleted. It's only necessary to be able to transfer TreeState ownership
    // Python -> Rust -> Python.
    pub fn destroy(self) -> TreeState {
        drop(self.filesystem);
        Rc::try_unwrap(self.treestate)
            .expect("Only a single reference to treestate left")
            .into_inner()
    }

    pub fn status(&self, matcher: Arc<dyn Matcher + Send + Sync + 'static>) -> Result<Status> {
        let pending_changes = self
            .filesystem
            .pending_changes(matcher.clone())?
            .filter_map(|result| match result {
                Ok(PendingChangeResult::File(change_type)) => {
                    match matcher.matches_file(change_type.get_path()) {
                        Ok(true) => Some(Ok(change_type)),
                        Err(e) => Some(Err(e)),
                        _ => None,
                    }
                }
                Err(e) => Some(Err(e)),
                _ => None,
            });

        compute_status(
            &*self.manifest.read(),
            self.treestate.clone(),
            pending_changes,
            matcher.clone(),
        )
    }

    pub fn copymap(&self) -> Result<Vec<(RepoPathBuf, RepoPathBuf)>> {
        self.treestate
            .borrow_mut()
            .visit_by_state(StateFlags::COPIED)?
            .into_iter()
            .map(|(path, state)| {
                let copied_path = state
                    .copied
                    .ok_or_else(|| anyhow!("Invalid treestate entry for {}: missing copied from path on file with COPIED flag", String::from_utf8_lossy(&path)))
                    .map(|p| p.into_vec())
                    .and_then(|p| RepoPathBuf::from_utf8(p).map_err(|e| anyhow!(e)))?;
                Ok((
                    RepoPathBuf::from_utf8(path).map_err(|e| anyhow!(e))?,
                    copied_path,
                ))
            })
            .collect()
    }
}
