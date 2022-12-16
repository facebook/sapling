/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use configloader::config::ConfigSet;
use configloader::Config;
use configmodel::ConfigExt;
use edenapi::Builder;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use hgcommits::DagCommits;
use metalog::MetaLog;
use parking_lot::Mutex;
use parking_lot::RwLock;
use repolock::RepoLocker;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStoreBuilder;
use revisionstore::trait_impls::ArcFileStore;
use revisionstore::EdenApiFileStore;
use revisionstore::EdenApiTreeStore;
use revisionstore::MemcacheStore;
use revsets::utils as revset_utils;
use storemodel::ReadFileContents;
use storemodel::RefreshableReadFileContents;
use storemodel::RefreshableTreeStore;
use storemodel::TreeStore;
use treestate::dirstate::Dirstate;
use treestate::dirstate::TreeStateFields;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use types::HgId;
use util::path::absolute;
use vfs::VFS;
use workingcopy::filesystem::FileSystemType;
use workingcopy::workingcopy::WorkingCopy;

use crate::commits::open_dag_commits;
use crate::errors;
use crate::init;
use crate::requirements::Requirements;
use crate::trees::TreeManifestResolver;

pub struct Repo {
    path: PathBuf,
    ident: identity::Identity,
    config: ConfigSet,
    shared_path: PathBuf,
    shared_ident: identity::Identity,
    store_path: PathBuf,
    dot_hg_path: PathBuf,
    shared_dot_hg_path: PathBuf,
    pub requirements: Requirements,
    pub store_requirements: Requirements,
    repo_name: Option<String>,
    metalog: Option<Arc<RwLock<MetaLog>>>,
    eden_api: Option<Arc<dyn EdenApi>>,
    dag_commits: Option<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>>,
    file_store: Option<Arc<dyn RefreshableReadFileContents<Error = anyhow::Error> + Send + Sync>>,
    tree_store: Option<Arc<dyn RefreshableTreeStore + Send + Sync>>,
    locker: Arc<RepoLocker>,
}

impl Repo {
    pub fn init(
        root_path: &Path,
        config: &ConfigSet,
        repo_config_contents: Option<String>,
        extra_config_values: &[String],
    ) -> Result<Repo> {
        let root_path = absolute(root_path)?;
        init::init_hg_repo(&root_path, config, repo_config_contents)?;
        let mut repo = Self::load(&root_path, extra_config_values, &[])?;
        repo.metalog()?.write().init_tracked()?;
        Ok(repo)
    }

    /// Load the repo from explicit path.
    ///
    /// Load repo configurations.
    pub fn load<P>(
        path: P,
        extra_config_values: &[String],
        extra_config_files: &[String],
    ) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        Self::build(path, extra_config_values, extra_config_files, None)
    }

    /// Loads the repo at given path, eschewing any config loading in
    /// favor of given config. This method exists so Python can create
    /// a Repo that uses the Python config verbatim without worrying
    /// about mixing CLI config overrides back in.
    pub fn load_with_config<P>(path: P, config: ConfigSet) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        Self::build(path, &[], &[], Some(config))
    }

    fn build<P>(
        path: P,
        extra_config_values: &[String],
        extra_config_files: &[String],
        config: Option<ConfigSet>,
    ) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        assert!(path.is_absolute());

        assert!(
            config.is_none() || (extra_config_values.is_empty() && extra_config_files.is_empty()),
            "Don't pass a config and CLI overrides to Repo::build"
        );

        let ident = match identity::sniff_dir(&path)? {
            Some(ident) => ident,
            None => {
                return Err(errors::RepoNotFound(path.to_string_lossy().to_string()).into());
            }
        };

        let config = match config {
            Some(config) => config,
            None => configloader::hg::load(Some(&path), extra_config_values, extra_config_files)?,
        };

        let dot_hg_path = path.join(ident.dot_dir());

        let (shared_path, shared_ident) = match read_sharedpath(&dot_hg_path)? {
            Some((path, ident)) => (path, ident),
            None => (path.clone(), ident.clone()),
        };
        let shared_dot_hg_path = shared_path.join(shared_ident.dot_dir());
        let store_path = shared_dot_hg_path.join("store");

        let repo_name = configloader::hg::read_repo_name_from_disk(&shared_dot_hg_path)
            .ok()
            .or_else(|| {
                config
                    .get("remotefilelog", "reponame")
                    .map(|v| v.to_string())
            });

        let requirements = Requirements::open(&dot_hg_path.join("requires"))?;
        let store_requirements = Requirements::open(&store_path.join("requires"))?;

        let locker = Arc::new(RepoLocker::new(&config, store_path.clone())?);

        Ok(Repo {
            path,
            ident,
            config,
            shared_path,
            shared_ident,
            store_path,
            dot_hg_path,
            shared_dot_hg_path,
            requirements,
            store_requirements,
            repo_name,
            metalog: None,
            eden_api: None,
            dag_commits: None,
            file_store: None,
            tree_store: None,
            locker,
        })
    }

    pub fn lock(&self) -> Result<repolock::RepoLockHandle, repolock::LockError> {
        self.locker.lock_store()
    }

    pub fn ensure_locked(&self) -> Result<(), repolock::LockError> {
        self.locker.ensure_store_locked()
    }

    pub fn reload_requires(&mut self) -> Result<()> {
        self.requirements = Requirements::open(&self.dot_hg_path.join("requires"))?;
        self.store_requirements = Requirements::open(&self.store_path.join("requires"))?;
        Ok(())
    }

    /// Return the store path.
    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    /// Return the shared repo root. If the repo is not shared, return the
    /// repo root.
    pub fn shared_path(&self) -> &Path {
        &self.shared_path
    }

    /// Repo root path, without `.hg`.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Repo root path, with `.hg`. Equivalent to self.path().join(".hg")
    pub fn dot_hg_path(&self) -> &Path {
        &self.dot_hg_path
    }

    /// Repo shared root path, with `.hg`. Equivalent to self.shared_path().join(".hg")
    pub fn shared_dot_hg_path(&self) -> &Path {
        &self.shared_dot_hg_path
    }

    pub fn config(&self) -> &ConfigSet {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut ConfigSet {
        &mut self.config
    }

    pub fn locker(&self) -> &Arc<RepoLocker> {
        &self.locker
    }

    pub fn repo_name(&self) -> Option<&str> {
        self.repo_name.as_ref().map(|s| s.as_ref())
    }

    pub fn config_path(&self) -> PathBuf {
        self.dot_hg_path.join(self.ident.config_repo_file())
    }

    pub fn metalog(&mut self) -> Result<Arc<RwLock<MetaLog>>> {
        match &self.metalog {
            Some(metalog) => Ok(metalog.clone()),
            None => {
                let metalog_path = self.metalog_path();
                let metalog = MetaLog::open_from_env(metalog_path.as_path())?;
                let metalog = Arc::new(RwLock::new(metalog));
                self.metalog = Some(metalog.clone());
                Ok(metalog)
            }
        }
    }

    pub fn invalidate_metalog(&mut self) {
        if self.metalog.is_some() {
            self.metalog = None;
        }
    }

    pub fn metalog_path(&self) -> PathBuf {
        self.store_path.join("metalog")
    }

    /// Constructs the EdenAPI client.
    ///
    /// This requires configs like `paths.default`. Avoid calling this function for
    /// local-only operations.
    pub fn eden_api(&mut self) -> Result<Arc<dyn EdenApi>, EdenApiError> {
        match &self.eden_api {
            Some(eden_api) => Ok(eden_api.clone()),
            None => {
                tracing::trace!(target: "repo::eden_api", "creating edenapi");
                let correlator = edenapi::DEFAULT_CORRELATOR.as_str();
                tracing::trace!(target: "repo::eden_api", "getting edenapi builder");
                let eden_api = Builder::from_config(&self.config)?
                    .correlator(Some(correlator))
                    .build()?;
                tracing::info!(url=eden_api.url(), path=?self.path, "EdenApi built");
                self.eden_api = Some(eden_api.clone());
                Ok(eden_api)
            }
        }
    }

    pub fn dag_commits(&mut self) -> Result<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>> {
        match &self.dag_commits {
            Some(commits) => Ok(commits.clone()),
            None => {
                let commits = open_dag_commits(self)?;
                let commits = Arc::new(RwLock::new(commits));
                self.dag_commits = Some(commits.clone());
                Ok(commits)
            }
        }
    }

    pub fn invalidate_dag_commits(&mut self) -> Result<()> {
        if let Some(dag_commits) = &mut self.dag_commits {
            let dag_commits = dag_commits.clone();
            let mut dag_commits = dag_commits.write();
            *dag_commits = open_dag_commits(self)?;
        }
        Ok(())
    }

    pub fn remote_bookmarks(&mut self) -> Result<BTreeMap<String, HgId>> {
        match self.metalog()?.read().get("remotenames")? {
            Some(rn) => Ok(refencode::decode_remotenames(&rn)?),
            None => Err(errors::RemotenamesMetalogKeyError.into()),
        }
    }

    pub fn add_requirement(&mut self, requirement: &str) -> Result<()> {
        self.requirements.add(requirement);
        self.requirements.flush()?;
        Ok(())
    }

    pub fn add_store_requirement(&mut self, requirement: &str) -> Result<()> {
        self.store_requirements.add(requirement);
        self.store_requirements.flush()?;
        Ok(())
    }

    pub fn file_store(
        &mut self,
    ) -> Result<Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>> {
        if let Some(fs) = &self.file_store {
            return Ok(Arc::new(fs.clone()));
        }

        tracing::trace!(target: "repo::file_store", "creating edenapi");
        let eden_api = if self.store_requirements.contains("git") {
            None
        } else {
            match self.eden_api() {
                Ok(eden_api) => Some(eden_api),
                // For tests, don't error if edenapi.url isn't set.
                Err(_) if std::env::var("TESTTMP").is_ok() => None,
                Err(e) => return Err(e.into()),
            }
        };

        tracing::trace!(target: "repo::file_store", "building filestore");
        let mut file_builder = FileStoreBuilder::new(self.config())
            .local_path(self.store_path())
            .correlator(edenapi::DEFAULT_CORRELATOR.as_str());

        if let Some(eden_api) = eden_api {
            file_builder = file_builder.edenapi(EdenApiFileStore::new(eden_api));
        } else {
            file_builder = file_builder.override_edenapi(false);
        }

        tracing::trace!(target: "repo::file_store", "configuring aux data");
        if self.config.get_or_default("scmstore", "auxindexedlog")? {
            file_builder = file_builder.store_aux_data();
        }

        tracing::trace!(target: "repo::file_store", "configuring memcache");
        if self
            .config
            .get_nonempty("remotefilelog", "cachekey")
            .is_some()
        {
            file_builder = file_builder.memcache(Arc::new(MemcacheStore::new(&self.config)?));
        }

        tracing::trace!(target: "repo::file_store", "building file store");
        let file_store = file_builder.build().context("when building FileStore")?;
        let fs = Arc::new(ArcFileStore(Arc::new(file_store)));

        self.file_store = Some(fs.clone());
        tracing::trace!(target: "repo::file_store", "filestore created");

        Ok(fs)
    }

    pub fn tree_store(&mut self) -> Result<Arc<dyn TreeStore + Send + Sync>> {
        if let Some(ts) = &self.tree_store {
            return Ok(Arc::new(ts.clone()));
        }

        let eden_api = if self.store_requirements.contains("git") {
            None
        } else {
            match self.eden_api() {
                Ok(eden_api) => Some(eden_api),
                // For tests, don't error if edenapi.url isn't set.
                Err(_) if std::env::var("TESTTMP").is_ok() => None,
                Err(e) => return Err(e.into()),
            }
        };

        let mut tree_builder = TreeStoreBuilder::new(self.config())
            .local_path(self.store_path())
            .suffix("manifests");

        if let Some(eden_api) = eden_api {
            tree_builder = tree_builder.edenapi(EdenApiTreeStore::new(eden_api));
        } else {
            tree_builder = tree_builder.override_edenapi(false);
        }
        let ts = Arc::new(tree_builder.build()?);
        self.tree_store = Some(ts.clone());
        Ok(ts)
    }

    pub fn tree_resolver(&mut self) -> Result<TreeManifestResolver> {
        Ok(TreeManifestResolver::new(
            self.dag_commits()?,
            self.tree_store()?,
        ))
    }

    pub fn resolve_commit(&mut self, treestate: &TreeState, change_id: &str) -> Result<HgId> {
        revset_utils::resolve_single(
            change_id,
            self.dag_commits()?.read().id_map_snapshot()?.as_ref(),
            &*self.metalog()?.read(),
            treestate,
        )
        .map_err(|e| e.into())
    }

    pub fn invalidate_stores(&mut self) -> Result<()> {
        if let Some(file_store) = &self.file_store {
            file_store.refresh()?;
        }
        if let Some(tree_store) = &self.tree_store {
            tree_store.refresh()?;
        }
        Ok(())
    }

    pub fn working_copy(&mut self, path: &Path) -> Result<WorkingCopy, errors::InvalidWorkingCopy> {
        let is_eden = self.requirements.contains("eden");
        let fsmonitor_ext = self.config.get("extensions", "fsmonitor");
        let fsmonitor_mode = self.config.get_nonempty("fsmonitor", "mode");
        let is_watchman = if fsmonitor_ext.is_none() || fsmonitor_ext == Some("!".into()) {
            false
        } else {
            fsmonitor_mode.is_none() || fsmonitor_mode == Some("on".into())
        };
        let filesystem = match (is_eden, is_watchman) {
            (true, _) => FileSystemType::Eden,
            (false, true) => FileSystemType::Watchman,
            (false, false) => FileSystemType::Normal,
        };

        tracing::trace!(target: "repo::workingcopy", "initializing vfs at {path:?}");
        let vfs = VFS::new(path.to_path_buf())?;
        let case_sensitive = vfs.case_sensitive();
        tracing::trace!(target: "repo::workingcopy", "case sensitive: {case_sensitive}");

        let dirstate_path = path.join(self.ident.dot_dir()).join("dirstate");
        tracing::trace!(target: "repo::workingcopy", dirstate_path=?dirstate_path);

        let treestate = match filesystem {
            FileSystemType::Eden => {
                tracing::trace!(target: "repo::workingcopy", "loading edenfs dirstate");
                TreeState::from_eden_dirstate(dirstate_path, case_sensitive)?
            }
            _ => {
                let treestate_path = path.join(self.ident.dot_dir()).join("treestate");
                if util::file::exists(&dirstate_path)
                    .map_err(anyhow::Error::from)?
                    .is_some()
                {
                    tracing::trace!(target: "repo::workingcopy", "reading dirstate file");
                    let mut buf =
                        util::file::open(dirstate_path, "r").map_err(anyhow::Error::from)?;
                    tracing::trace!(target: "repo::workingcopy", "deserializing dirstate");
                    let dirstate = Dirstate::deserialize(&mut buf)?;
                    let fields = dirstate
                        .tree_state
                        .ok_or_else(|| anyhow!("missing treestate fields on dirstate"))?;

                    let filename = fields.tree_filename;
                    let root_id = fields.tree_root_id;
                    tracing::trace!(target: "repo::workingcopy", "loading treestate {filename} {root_id:?}");
                    TreeState::open(treestate_path.join(filename), root_id, case_sensitive)?
                } else {
                    tracing::trace!(target: "repo::workingcopy", "creating treestate");
                    let (treestate, root_id) = TreeState::new(&treestate_path, case_sensitive)?;

                    tracing::trace!(target: "repo::workingcopy", "creating dirstate");
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
        tracing::trace!(target: "repo::workingcopy", "treestate loaded");
        let treestate = Arc::new(Mutex::new(treestate));

        tracing::trace!(target: "repo::workingcopy", "creating file store");
        let file_store = self.file_store()?;

        tracing::trace!(target: "repo::workingcopy", "creating tree resolver");
        let tree_resolver = Arc::new(self.tree_resolver()?);

        Ok(WorkingCopy::new(
            vfs,
            filesystem,
            treestate,
            tree_resolver,
            file_store,
            &self.config,
            self.locker.clone(),
        )?)
    }

    async fn get_root_tree_id(&mut self, commit_id: HgId) -> Result<HgId> {
        let commit_store = self.dag_commits()?.read().to_dyn_read_root_tree_ids();
        let tree_ids = commit_store.read_root_tree_ids(vec![commit_id]).await?;
        Ok(tree_ids[0].1)
    }
}

fn read_sharedpath(dot_path: &Path) -> Result<Option<(PathBuf, identity::Identity)>> {
    let sharedpath = fs::read_to_string(dot_path.join("sharedpath"))
        .ok()
        .map(|s| PathBuf::from(s))
        .and_then(|p| Some(PathBuf::from(p.parent()?)));

    if let Some(mut possible_path) = sharedpath {
        // sharedpath can be relative to our dot dir.
        possible_path = dot_path.join(possible_path);

        if !possible_path.is_dir() {
            return Err(
                errors::InvalidSharedPath(possible_path.to_string_lossy().to_string()).into(),
            );
        }

        return match identity::sniff_dir(&possible_path)? {
            Some(ident) => Ok(Some((possible_path, ident))),
            None => {
                Err(errors::InvalidSharedPath(possible_path.to_string_lossy().to_string()).into())
            }
        };
    }

    Ok(None)
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("path", &self.path)
            .field("repo_name", &self.repo_name)
            .finish()
    }
}
