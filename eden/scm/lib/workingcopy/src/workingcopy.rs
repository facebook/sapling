/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
#[cfg(feature = "eden")]
use edenfs_client::EdenFsClient;
use identity::Identity;
use io::IO;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use parking_lot::RwLock;
use pathmatcher::DifferenceMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::GitignoreMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::Matcher;
use pathmatcher::NegateMatcher;
use pathmatcher::UnionMatcher;
use repolock::RepoLocker;
use status::FileStatus;
use status::Status;
use status::StatusBuilder;
use storemodel::FileStore;
use treestate::filestate::StateFlags;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::repo::StorageFormat;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::VFS;

#[cfg(feature = "eden")]
use crate::edenfs::EdenFileSystem;
use crate::errors;
use crate::filesystem::FileSystem;
use crate::filesystem::FileSystemType;
use crate::filesystem::PendingChange;
use crate::git::parse_submodules;
use crate::physicalfs::PhysicalFileSystem;
use crate::status::compute_status;
use crate::util::walk_treestate;
use crate::watchmanfs::WatchmanFileSystem;

type ArcFileStore = Arc<dyn FileStore>;
type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;
type BoxFileSystem = Box<dyn FileSystem + Send>;

pub struct WorkingCopy {
    vfs: VFS,
    ident: Identity,
    format: StorageFormat,
    treestate: Arc<Mutex<TreeState>>,
    tree_resolver: ArcReadTreeManifest,
    filestore: ArcFileStore,
    pub(crate) filesystem: Mutex<BoxFileSystem>,
    ignore_matcher: Arc<GitignoreMatcher>,
    locker: Arc<RepoLocker>,
    dot_hg_path: PathBuf,
}

impl WorkingCopy {
    pub fn new(
        vfs: VFS,
        format: StorageFormat,
        // TODO: Have constructor figure out FileSystemType
        file_system_type: FileSystemType,
        treestate: Arc<Mutex<TreeState>>,
        tree_resolver: ArcReadTreeManifest,
        filestore: ArcFileStore,
        config: &dyn Config,
        locker: Arc<RepoLocker>,
    ) -> Result<Self> {
        tracing::debug!(target: "dirstate_size", dirstate_size=treestate.lock().len());

        let ignore_matcher = Arc::new(GitignoreMatcher::new(
            vfs.root(),
            WorkingCopy::global_ignore_paths(vfs.root(), config)
                .iter()
                .map(|i| i.as_path())
                .collect(),
            vfs.case_sensitive(),
        ));

        let filesystem = Mutex::new(Self::construct_file_system(
            vfs.clone(),
            file_system_type,
            treestate.clone(),
            tree_resolver.clone(),
            filestore.clone(),
            locker.clone(),
        )?);

        let root = vfs.root();
        let ident = match identity::sniff_dir(root)? {
            Some(ident) => ident,
            None => {
                return Err(errors::RepoNotFound(root.to_string_lossy().to_string()).into());
            }
        };
        let dot_hg_path = vfs.join(RepoPath::from_str(ident.dot_dir())?);

        Ok(WorkingCopy {
            vfs,
            format,
            ident,
            treestate,
            tree_resolver,
            filestore,
            filesystem,
            ignore_matcher,
            locker,
            dot_hg_path,
        })
    }

    /// Working copy root path, with `.hg`.
    pub fn dot_hg_path(&self) -> &Path {
        &self.dot_hg_path
    }

    pub fn lock(&self) -> Result<repolock::RepoLockHandle, repolock::LockError> {
        self.locker.lock_working_copy(self.dot_hg_path.clone())
    }

    pub fn ensure_locked(&self) -> Result<(), repolock::LockError> {
        self.locker.ensure_working_copy_locked(&self.dot_hg_path)
    }

    pub fn treestate(&self) -> Arc<Mutex<TreeState>> {
        self.treestate.clone()
    }

    pub fn vfs(&self) -> &VFS {
        &self.vfs
    }

    pub fn parents(&self) -> Result<Vec<HgId>> {
        self.treestate.lock().parents().collect()
    }

    pub fn set_parents(&mut self, parents: &mut dyn Iterator<Item = &HgId>) -> Result<()> {
        self.treestate.lock().set_parents(parents)
    }

    pub fn filestore(&self) -> ArcFileStore {
        self.filestore.clone()
    }

    pub fn tree_resolver(&self) -> ArcReadTreeManifest {
        self.tree_resolver.clone()
    }

    pub(crate) fn current_manifests(
        treestate: &TreeState,
        tree_resolver: &ArcReadTreeManifest,
    ) -> Result<Vec<Arc<RwLock<TreeManifest>>>> {
        let mut parents = treestate.parents().peekable();
        if parents.peek_mut().is_some() {
            parents.map(|p| tree_resolver.get(&p?)).collect()
        } else {
            let null_commit = HgId::null_id().clone();
            Ok(vec![
                tree_resolver
                    .get(&null_commit)
                    .context("resolving null commit tree")?,
            ])
        }
    }

    fn global_ignore_paths(root: &Path, config: &dyn Config) -> Vec<PathBuf> {
        config
            .keys_prefixed("ui", "ignore.")
            .iter()
            .chain(Some(&"ignore".into()))
            .filter_map(
                |name| match config.get_nonempty_opt::<PathBuf>("ui", name) {
                    Ok(Some(path)) => Some(root.join(path)),
                    _ => None,
                },
            )
            .collect()
    }

    fn construct_file_system(
        vfs: VFS,
        file_system_type: FileSystemType,
        treestate: Arc<Mutex<TreeState>>,
        tree_resolver: ArcReadTreeManifest,
        store: ArcFileStore,
        locker: Arc<RepoLocker>,
    ) -> Result<BoxFileSystem> {
        Ok(match file_system_type {
            FileSystemType::Normal => Box::new(PhysicalFileSystem::new(
                vfs.clone(),
                tree_resolver,
                store.clone(),
                treestate,
                locker,
            )?),
            FileSystemType::Watchman => Box::new(WatchmanFileSystem::new(
                vfs.clone(),
                tree_resolver,
                store.clone(),
                treestate,
                locker,
            )?),
            FileSystemType::Eden => {
                #[cfg(not(feature = "eden"))]
                panic!("cannot use EdenFS in a non-EdenFS build");
                #[cfg(feature = "eden")]
                {
                    let wdir = vfs.root();
                    let client = EdenFsClient::from_wdir(wdir)?;
                    Box::new(EdenFileSystem::new(treestate, client)?)
                }
            }
        })
    }

    fn added_files(&self) -> Result<Vec<RepoPathBuf>> {
        let mut added_files: Vec<RepoPathBuf> = vec![];
        self.treestate.lock().visit(
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

    pub fn status(
        &self,
        mut matcher: DynMatcher,
        last_write: SystemTime,
        include_ignored: bool,
        config: &dyn Config,
        io: &IO,
    ) -> Result<Status> {
        let added_files = self.added_files()?;

        let manifests =
            WorkingCopy::current_manifests(&self.treestate.lock(), &self.tree_resolver)?;
        let mut manifest_matchers: Vec<DynMatcher> = Vec::with_capacity(manifests.len());

        let case_sensitive = self.vfs.case_sensitive();

        for manifest in manifests.iter() {
            manifest_matchers.push(Arc::new(manifest_tree::ManifestMatcher::new(
                manifest.clone(),
                case_sensitive,
            )));
        }

        let sparse_matcher = self
            .filesystem
            .lock()
            .sparse_matcher(&manifests, self.ident.dot_dir())?;

        if let Some(sparse) = sparse_matcher.clone() {
            matcher = Arc::new(IntersectMatcher::new(vec![matcher, sparse]));
        }

        // The GitignoreMatcher minus files in the repo. In other words, it does
        // not match an ignored file that has been previously committed.
        let mut ignore_matcher: DynMatcher = Arc::new(DifferenceMatcher::new(
            self.ignore_matcher.clone(),
            UnionMatcher::new(manifest_matchers),
        ));

        // If we have been asked to report ignored files, don't skip them in the matcher.
        if !include_ignored {
            matcher = Arc::new(DifferenceMatcher::new(matcher, ignore_matcher.clone()));
        }

        // Treat files outside sparse profile as ignored.
        if let Some(sparse) = sparse_matcher.clone() {
            ignore_matcher = Arc::new(UnionMatcher::new(vec![
                ignore_matcher,
                Arc::new(NegateMatcher::new(sparse)),
            ]));
        }

        let mut ignore_dirs = vec![PathBuf::from(self.ident.dot_dir())];
        if self.format.is_git() {
            // Ignore file within submodules. Python has some logic additional
            // logic layered on top to add submodule info into status results.
            let git_modules_path = self.vfs.join(".gitmodules".try_into()?);
            if git_modules_path.exists() {
                ignore_dirs.extend(
                    parse_submodules(&fs_err::read(&git_modules_path)?)?
                        .into_iter()
                        .map(|s| PathBuf::from(s.path)),
                );
            }
        }

        let pending_changes = self
            .filesystem
            .lock()
            .pending_changes(
                matcher.clone(),
                ignore_matcher,
                ignore_dirs,
                include_ignored,
                last_write,
                config,
                io,
            )?
            .filter_map(|result| match result {
                Ok(change_type) => match matcher.matches_file(change_type.get_path()) {
                    Ok(true) => {
                        tracing::trace!(?change_type, "pending change");
                        Some(Ok(change_type))
                    }
                    Err(e) => Some(Err(e)),
                    _ => None,
                },
                Err(e) => Some(Err(e)),
            })
            // fs.pending_changes() won't return ignored files, but we want added ignored files to
            // show up in the results, so let's inject them here.
            .chain(added_files.into_iter().filter_map(|path| {
                match self.ignore_matcher.matches_file(&path) {
                    Ok(result) if result => match self.vfs.metadata(&path) {
                        Ok(ref attr) if attr.is_dir() => None,
                        Ok(_) => Some(Ok(PendingChange::Changed(path))),
                        Err(err) => {
                            if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
                                // If file is not on disk, report as deleted so it shows up as "!".
                                if io_err.kind() == std::io::ErrorKind::NotFound {
                                    return Some(Ok(PendingChange::Deleted(path)));
                                }
                            }

                            // Propagate error otherwise this added file might disappear from "status".
                            Some(Err(err))
                        }
                    },
                    Ok(_) => None,
                    Err(e) => Some(Err(e)),
                }
            }));

        let p1_manifest = &*manifests[0].read();
        let mut status_builder = compute_status(
            p1_manifest,
            self.treestate.clone(),
            pending_changes,
            matcher.clone(),
        )?;

        if !self.vfs.supports_symlinks()
            && config
                .get_or_default("unsafe", "filtersuspectsymlink")
                .unwrap_or_default()
        {
            status_builder =
                self.filter_accidential_symlink_changes(status_builder, p1_manifest)?;
        }

        Ok(status_builder.build())
    }

    // Filter out modified symlinks where it appears the symlink has
    // been modified to no longer be a symlink. This happens often on
    // Windows because we don't materialize symlinks in the working
    // copy. The comment in Python's _filtersuspectsymlink suggests it
    // can also happen on network mounts.
    fn filter_accidential_symlink_changes(
        &self,
        mut status_builder: StatusBuilder,
        manifest: &impl Manifest,
    ) -> Result<StatusBuilder> {
        let mut override_clean = Vec::new();
        for (path, status) in status_builder.iter() {
            if status != FileStatus::Modified {
                continue;
            }

            let file_metadata = match manifest.get_file(path)? {
                Some(md) => md,
                None => continue,
            };

            if file_metadata.file_type != FileType::Symlink {
                continue;
            }

            let data = self.vfs.read(path)?;
            if data.is_empty() || data.len() >= 1024 || data.iter().any(|b| *b == b'\n' || *b == 0)
            {
                override_clean.push(path.to_owned());
            }
        }

        if !override_clean.is_empty() {
            status_builder = status_builder.clean(override_clean)
        }

        Ok(status_builder)
    }

    pub fn copymap(&self, matcher: DynMatcher) -> Result<Vec<(RepoPathBuf, RepoPathBuf)>> {
        let mut copied: Vec<(RepoPathBuf, RepoPathBuf)> = Vec::new();

        walk_treestate(
            &mut self.treestate.lock(),
            matcher,
            StateFlags::COPIED,
            StateFlags::empty(),
            |path, state| {
                let copied_path = state
                    .copied
                    .clone()
                    .ok_or_else(|| anyhow!("Invalid treestate entry for {}: missing copied from path on file with COPIED flag", path))
                    .map(|p| p.into_vec())
                    .and_then(|p| RepoPathBuf::from_utf8(p).map_err(|e| anyhow!(e)))?;

                copied.push((path, copied_path));

                Ok(())
            },
        )?;

        Ok(copied)
    }
}
