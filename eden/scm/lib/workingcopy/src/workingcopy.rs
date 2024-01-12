/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
#[cfg(feature = "eden")]
use edenfs_client::EdenFsClient;
use identity::Identity;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DifferenceMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::GitignoreMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::Matcher;
use pathmatcher::NegateMatcher;
use pathmatcher::UnionMatcher;
use repolock::LockedPath;
use repolock::RepoLocker;
use repostate::MergeState;
use status::FileStatus;
use status::Status;
use status::StatusBuilder;
use storemodel::FileStore;
use termlogger::TermLogger;
use treestate::dirstate::Dirstate;
use treestate::dirstate::TreeStateFields;
use treestate::filestate::StateFlags;
use treestate::serialization::Serializable;
use treestate::tree::VisitorResult;
use treestate::treestate::TreeState;
use types::repo::StorageFormat;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;
use util::file::atomic_write;
use util::file::read_to_string_if_exists;
use util::file::unlink_if_exists;
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

#[cfg(not(feature = "eden"))]
pub struct EdenFsClient {}

#[cfg(not(feature = "eden"))]
impl EdenFsClient {
    pub fn from_wdir(_wdir_root: &Path) -> anyhow::Result<Self> {
        panic!("cannot use EdenFS in a non-EdenFS build");
    }
}

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
    pub(crate) locker: Arc<RepoLocker>,
    pub(crate) dot_hg_path: PathBuf,
    eden_client: Option<Arc<EdenFsClient>>,
}

const ACTIVE_BOOKMARK_FILE: &str = "bookmarks.current";

impl WorkingCopy {
    pub fn new(
        path: &Path,
        config: &dyn Config,
        format: StorageFormat,
        tree_resolver: ArcReadTreeManifest,
        filestore: ArcFileStore,
        locker: Arc<RepoLocker>,
        // For dirstate
        dot_dir: &Path,
        has_requirement: &dyn Fn(&str) -> bool,
    ) -> Result<Self> {
        tracing::trace!("initializing vfs at {path:?}");
        let vfs = VFS::new(path.to_path_buf())?;

        let is_eden = has_requirement("eden");

        // In case the "requires" file gets corrupted, check `.eden` directory
        // and prevent treating edenfs as non-edenfs.
        if !is_eden && path.join(".eden").is_dir() {
            anyhow::bail!(
                "Detected conflicting information about whether EdenFS is enabled.\n\
                 This might indicate repo metadata (ex. {}) corruption.\n\
                 To avoid further corruption, this is a fatal error.\n\
                 Contact the Source Control support team for investigation.",
                dot_dir.join("requires").display()
            );
        }

        let fsmonitor_ext = config.get("extensions", "fsmonitor");
        let fsmonitor_mode = config.get_nonempty("fsmonitor", "mode");
        let is_watchman = if fsmonitor_ext.is_none() || fsmonitor_ext == Some("!".into()) {
            false
        } else {
            fsmonitor_mode.is_none() || fsmonitor_mode == Some("on".into())
        };
        let file_system_type = match (is_eden, is_watchman) {
            (true, _) => FileSystemType::Eden,
            (false, true) => FileSystemType::Watchman,
            (false, false) => FileSystemType::Normal,
        };
        let treestate = {
            let case_sensitive = vfs.case_sensitive();
            tracing::trace!("case sensitive: {case_sensitive}");
            let dirstate_path = dot_dir.join("dirstate");
            let treestate = match file_system_type {
                FileSystemType::Eden => {
                    tracing::trace!("loading edenfs dirstate");
                    TreeState::from_eden_dirstate(dirstate_path, case_sensitive)?
                }
                _ => {
                    let treestate_path = dot_dir.join("treestate");
                    if util::file::exists(&dirstate_path)
                        .map_err(anyhow::Error::from)?
                        .is_some()
                    {
                        tracing::trace!("reading dirstate file");
                        let mut buf =
                            util::file::open(dirstate_path, "r").map_err(anyhow::Error::from)?;
                        tracing::trace!("deserializing dirstate");
                        let dirstate = Dirstate::deserialize(&mut buf)?;
                        let fields = dirstate
                            .tree_state
                            .ok_or_else(|| anyhow!("missing treestate fields on dirstate"))?;

                        let filename = fields.tree_filename;
                        let root_id = fields.tree_root_id;
                        tracing::trace!("loading treestate {filename} {root_id:?}");
                        TreeState::open(treestate_path.join(filename), root_id, case_sensitive)?
                    } else {
                        tracing::trace!("creating treestate");
                        let (treestate, root_id) = TreeState::new(&treestate_path, case_sensitive)?;

                        tracing::trace!("creating dirstate");
                        let dirstate = Dirstate {
                            p1: *HgId::null_id(),
                            p2: *HgId::null_id(),
                            tree_state: Some(TreeStateFields {
                                tree_filename: treestate.file_name()?,
                                tree_root_id: root_id,
                                // TODO: set threshold
                                repack_threshold: None,
                            }),
                        };

                        tracing::trace!(target: "repo::workingcopy", "creating dirstate file");
                        let mut file =
                            util::file::create(dirstate_path).map_err(anyhow::Error::from)?;

                        tracing::trace!(target: "repo::workingcopy", "serializing dirstate");
                        dirstate.serialize(&mut file)?;
                        treestate
                    }
                }
            };
            tracing::debug!(target: "dirstate_size", dirstate_size=treestate.len());
            Arc::new(Mutex::new(treestate))
        };

        let ignore_matcher = Arc::new(GitignoreMatcher::new(
            vfs.root(),
            WorkingCopy::global_ignore_paths(vfs.root(), config)
                .iter()
                .map(|i| i.as_path())
                .collect(),
            vfs.case_sensitive(),
        ));

        let (filesystem, eden_client) = Self::construct_file_system(
            vfs.clone(),
            file_system_type,
            treestate.clone(),
            tree_resolver.clone(),
            filestore.clone(),
            locker.clone(),
        )?;
        let filesystem = Mutex::new(filesystem);

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
            eden_client,
        })
    }

    /// Working copy root path, with `.hg`.
    pub fn dot_hg_path(&self) -> &Path {
        &self.dot_hg_path
    }

    pub fn lock(&self) -> Result<LockedWorkingCopy, repolock::LockError> {
        let locked_path = self.locker.lock_working_copy(self.dot_hg_path.clone())?;
        Ok(LockedWorkingCopy {
            dot_hg_path: locked_path,
            wc: self,
        })
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

    pub fn filestore(&self) -> ArcFileStore {
        self.filestore.clone()
    }

    pub fn tree_resolver(&self) -> ArcReadTreeManifest {
        self.tree_resolver.clone()
    }

    pub(crate) fn current_manifests(
        treestate: &TreeState,
        tree_resolver: &ArcReadTreeManifest,
    ) -> Result<Vec<Arc<TreeManifest>>> {
        let mut parents = treestate.parents().peekable();
        if parents.peek_mut().is_some() {
            parents
                .map(|p| Ok(Arc::new(tree_resolver.get(&p?)?)))
                .collect()
        } else {
            let null_commit = HgId::null_id().clone();
            Ok(vec![Arc::new(
                tree_resolver
                    .get(&null_commit)
                    .context("resolving null commit tree")?,
            )])
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
    ) -> Result<(BoxFileSystem, Option<Arc<EdenFsClient>>)> {
        Ok(match file_system_type {
            FileSystemType::Normal => (
                Box::new(PhysicalFileSystem::new(
                    vfs.clone(),
                    tree_resolver,
                    store.clone(),
                    treestate,
                    locker,
                )?),
                None,
            ),
            FileSystemType::Watchman => (
                Box::new(WatchmanFileSystem::new(
                    vfs.clone(),
                    tree_resolver,
                    store.clone(),
                    treestate,
                    locker,
                )?),
                None,
            ),
            FileSystemType::Eden => {
                #[cfg(not(feature = "eden"))]
                panic!("cannot use EdenFS in a non-EdenFS build");
                #[cfg(feature = "eden")]
                {
                    let client = Arc::new(EdenFsClient::from_wdir(vfs.root())?);
                    (
                        Box::new(EdenFileSystem::new(
                            treestate,
                            client.clone(),
                            vfs.clone(),
                            store.clone(),
                        )?),
                        Some(client),
                    )
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
        include_ignored: bool,
        config: &dyn Config,
        lgr: &TermLogger,
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
                config,
                lgr,
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

        let p1_manifest = manifests[0].as_ref();
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

    pub fn eden_client(&self) -> Result<Arc<EdenFsClient>> {
        self.eden_client
            .clone()
            .context("EdenFS client not available in current working copy")
    }

    pub fn read_merge_state(&self) -> Result<Option<MergeState>> {
        // Conceptually it seems like read_merge_state should be on LockedWorkingCopy.
        // In practice, light weight operations such as status+morestatus read the
        // merge state without a lock, so we can't require a lock. The merge
        // state is written atomically so we won't see an incomplete merge
        // state, but if we read other state files without locking then things
        // can be inconsistent.

        MergeState::read(&self.dot_hg_path().join("merge/state2"))
    }

    pub fn active_bookmark(&self) -> Result<Option<String>> {
        Ok(read_to_string_if_exists(
            self.dot_hg_path.join(ACTIVE_BOOKMARK_FILE),
        )?)
    }
}

pub struct LockedWorkingCopy<'a> {
    dot_hg_path: LockedPath,
    wc: &'a WorkingCopy,
}

impl<'a> std::ops::Deref for LockedWorkingCopy<'a> {
    type Target = WorkingCopy;

    fn deref(&self) -> &Self::Target {
        self.wc
    }
}

impl<'a> LockedWorkingCopy<'a> {
    pub fn locked_dot_hg_path(&self) -> &LockedPath {
        &self.dot_hg_path
    }

    pub fn write_merge_state(&self, ms: &MergeState) -> Result<()> {
        let dir = self.dot_hg_path.join("merge");
        fs_err::create_dir_all(&dir)?;
        let mut f = util::file::atomic_open(&dir.join("state2"))?;
        ms.serialize(f.as_file())?;
        f.save()?;
        Ok(())
    }

    pub fn set_parents(&self, parents: Vec<HgId>, parent_tree_hash: Option<HgId>) -> Result<()> {
        let p1 = parents
            .get(0)
            .context("At least one parent is required for setting parents")?
            .clone();
        let p2 = parents.get(1).copied();
        self.treestate.lock().set_parents(&mut parents.iter())?;
        self.filesystem.lock().set_parents(p1, p2, parent_tree_hash)
    }

    pub fn clear_merge_state(&self) -> Result<()> {
        let merge_state_dir = self.dot_hg_path().join("merge");
        if util::file::exists(&merge_state_dir)
            .context("clearing merge state")?
            .is_some()
        {
            fs_err::remove_dir_all(&merge_state_dir)?;
        }
        Ok(())
    }

    pub fn set_active_bookmark(&self, bm: Option<String>) -> Result<()> {
        let active_path = self.dot_hg_path.join(ACTIVE_BOOKMARK_FILE);
        match bm {
            Some(bm) => Ok(atomic_write(&active_path, |f| write!(f, "{bm}")).map(|_f| ())?),
            None => Ok(unlink_if_exists(&active_path)?),
        }
    }
}
