/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use configmodel::Config;
use configparser::config::ConfigSet;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::RwLock;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DifferenceMatcher;
use pathmatcher::ExactMatcher;
use pathmatcher::GitignoreMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::Matcher;
use pathmatcher::UnionMatcher;
use status::Status;
use storemodel::ReadFileContents;
use treestate::filestate::StateFlags;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::HgId;
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
type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;
type FileSystem = Box<dyn PendingChanges>;

pub struct WorkingCopy {
    treestate: Rc<RefCell<TreeState>>,
    manifests: Vec<Arc<RwLock<TreeManifest>>>,
    filesystem: FileSystem,
    ignore_matcher: Arc<GitignoreMatcher>,
    sparse_matcher: Arc<dyn Matcher + Send + Sync + 'static>,
}

impl WorkingCopy {
    pub fn new(
        root: PathBuf,
        // TODO: Have constructor figure out FileSystemType
        file_system_type: FileSystemType,
        treestate: TreeState,
        tree_resolver: ArcReadTreeManifest,
        filestore: ArcReadFileContents,
        last_write: SystemTime,
        config: &ConfigSet,
    ) -> std::result::Result<Self, (TreeState, Error)> {
        tracing::debug!(target: "dirstate_size", dirstate_size=treestate.len());

        let manifests = match WorkingCopy::current_manifests(&treestate, &tree_resolver) {
            Ok(manifests) => manifests,
            Err(e) => {
                return Err((treestate, e));
            }
        };

        let ignore_matcher = Arc::new(GitignoreMatcher::new(
            &root,
            WorkingCopy::global_ignore_paths(&root, config)
                .iter()
                .map(|i| i.as_path())
                .collect(),
        ));

        // We assume there will be at least one manifest, even if it's the null manifest.
        assert!(!manifests.is_empty());

        let mut sparse_matchers: Vec<Arc<dyn Matcher + Send + Sync + 'static>> = Vec::new();
        if file_system_type == FileSystemType::Eden {
            sparse_matchers.push(Arc::new(AlwaysMatcher::new()));
        } else {
            let ident = match identity::must_sniff_dir(&root) {
                Ok(ident) => ident,
                Err(err) => return Err((treestate, err)),
            };
            for manifest in manifests.iter() {
                match crate::sparse::repo_matcher(
                    &root.join(ident.dot_dir()),
                    manifest.read().clone(),
                    filestore.clone(),
                ) {
                    Ok(Some(matcher)) => {
                        sparse_matchers.push(matcher);
                    }
                    Ok(None) => {
                        sparse_matchers.push(Arc::new(AlwaysMatcher::new()));
                    }
                    Err(err) => return Err((treestate, err)),
                };
            }
        }

        let treestate = Rc::new(RefCell::new(treestate));

        let p1_manifest = manifests[0].clone();

        let filesystem: Result<FileSystem> = Self::construct_file_system(
            root.clone(),
            file_system_type,
            treestate.clone(),
            p1_manifest,
            filestore,
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
            manifests,
            filesystem,
            ignore_matcher,
            sparse_matcher: Arc::new(UnionMatcher::new(sparse_matchers)),
        })
    }

    fn current_manifests(
        treestate: &TreeState,
        tree_resolver: &ArcReadTreeManifest,
    ) -> Result<Vec<Arc<RwLock<TreeManifest>>>> {
        let mut parents = vec![];
        let mut i = 1;
        loop {
            match treestate.get_metadata_by_key(format!("p{}", i).as_str())? {
                Some(s) => parents.push(HgId::from_str(&s)?),
                None => break,
            };
            i += 1;
        }
        if parents.is_empty() {
            parents.push(*HgId::null_id());
        }

        parents.iter().map(|p| tree_resolver.get(p)).collect()
    }

    fn global_ignore_paths(root: &Path, config: &ConfigSet) -> Vec<PathBuf> {
        let mut ignore_paths = vec![];
        if let Some(value) = config.get("ui", "ignore") {
            let path = Path::new(value.as_ref());
            ignore_paths.push(root.join(path));
        }
        for name in config.keys_prefixed("ui", "ignore.") {
            let value = config.get("ui", &name).unwrap();
            let path = Path::new(value.as_ref());
            ignore_paths.push(root.join(path));
        }
        ignore_paths
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

    fn added_files(&self) -> Result<Vec<RepoPathBuf>> {
        let mut added_files: Vec<RepoPathBuf> = vec![];
        self.treestate.borrow_mut().visit(
            &mut |components, _| {
                let path = components.concat();
                let path = RepoPathBuf::from_utf8(path)?;
                added_files.push(path);
                Ok(VisitorResult::NotChanged)
            },
            &|_path, dir| match dir.get_aggregated_state() {
                None => true,
                Some(state) => {
                    let any_not_exists_parent = !state
                        .intersection
                        .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2);
                    let any_exists_next = state.union.intersects(StateFlags::EXIST_NEXT);
                    any_not_exists_parent && any_exists_next
                }
            },
            &|_path, file| {
                !file
                    .state
                    .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2)
                    && file.state.intersects(StateFlags::EXIST_NEXT)
            },
        )?;
        Ok(added_files)
    }

    pub fn status(&self, matcher: Arc<dyn Matcher + Send + Sync + 'static>) -> Result<Status> {
        let added_files = self.added_files()?;
        let mut non_ignore_matchers: Vec<Arc<dyn Matcher + Send + Sync + 'static>> =
            Vec::with_capacity(self.manifests.len());
        for manifest in self.manifests.iter() {
            non_ignore_matchers.push(Arc::new(manifest_tree::ManifestMatcher::new(
                manifest.clone(),
            )));
        }
        non_ignore_matchers.push(Arc::new(ExactMatcher::new(added_files.iter())));

        let matcher = Arc::new(IntersectMatcher::new(vec![
            matcher,
            self.sparse_matcher.clone(),
        ]));

        let matcher = Arc::new(DifferenceMatcher::new(
            matcher,
            DifferenceMatcher::new(
                self.ignore_matcher.clone(),
                UnionMatcher::new(non_ignore_matchers),
            ),
        ));
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

        let p1_manifest = &*self.manifests[0].read();
        compute_status(
            p1_manifest,
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
