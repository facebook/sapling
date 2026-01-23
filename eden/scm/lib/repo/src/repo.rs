/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use commits_trait::DagCommits;
use configloader::Config;
use configloader::config::ConfigSet;
use configloader::hg::PinnedConfig;
use configloader::hg::RepoInfo;
use eagerepo::EagerRepoStore;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use identity::Identity;
use manifest_tree::ReadTreeManifest;
use metalog::MetaLog;
use metalog::RefName;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use repo_minimal_info::RepoMinimalInfo;
use repo_minimal_info::Requirements;
pub use repo_minimal_info::read_sharedpath;
use repolock::RepoLocker;
use revisionstore::scmstore;
use revisionstore::trait_impls::ArcFileStore;
use revsets::errors::RevsetLookupError;
use revsets::utils as revset_utils;
use rewrite_macros::cached_field;
use storemodel::FileStore;
use storemodel::StoreInfo;
use storemodel::StoreOutput;
use storemodel::TreeStore;
use treestate::treestate::TreeState;
use types::HgId;
use types::repo::StorageFormat;
use util::path::absolute;
#[cfg(feature = "wdir")]
use workingcopy::workingcopy::WorkingCopy;

use crate::errors;
use crate::init;
use crate::scmstore::build_scm_file_store;
use crate::scmstore::build_scm_tree_store;
use crate::slapi_client::LazyCapabilities;
use crate::slapi_client::get_eden_api;
use crate::slapi_client::get_eden_api_with_capabilities;
use crate::slapi_client::get_optional_eden_api;
use crate::trees::TreeManifestResolver;

#[derive(Clone)]
pub struct Repo {
    info: RepoMinimalInfo,
    config: Arc<dyn Config>,
    repo_name: Option<String>,
    metalog: OnceCell<Arc<RwLock<MetaLog>>>,
    eden_api: OnceCell<(LazyCapabilities, Arc<dyn SaplingRemoteApi>)>,
    dag_commits: OnceCell<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>>,
    file_store: OnceCell<Arc<dyn FileStore>>,
    file_scm_store: OnceCell<Arc<scmstore::FileStore>>,
    tree_store: OnceCell<Arc<dyn TreeStore>>,
    tree_scm_store: OnceCell<Arc<scmstore::TreeStore>>,
    #[cfg(feature = "wdir")]
    working_copy: OnceCell<Arc<RwLock<WorkingCopy>>>,
    eager_store: Option<EagerRepoStore>,
    locker: Arc<RepoLocker>,
    tree_resolver: OnceCell<Arc<dyn ReadTreeManifest>>,
}

impl Deref for Repo {
    type Target = RepoMinimalInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl Repo {
    pub fn init(
        root_path: &Path,
        config: &ConfigSet,
        repo_config_contents: Option<String>,
        pinned_config: &[PinnedConfig],
    ) -> Result<Repo> {
        let root_path = absolute(root_path)?;
        init::init_hg_repo(&root_path, config, repo_config_contents)?;
        let repo = Self::load(&root_path, pinned_config)?;
        repo.metalog()?.write().init_tracked()?;
        Ok(repo)
    }

    /// Load the repo from explicit path.
    ///
    /// Load repo configurations.
    pub fn load<P>(path: P, pinned_config: &[PinnedConfig]) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        Self::build(path.into(), pinned_config, None)
    }

    /// Loads the repo at given path, eschewing any config loading in
    /// favor of given config. This method exists so Python can create
    /// a Repo that uses the Python config verbatim without worrying
    /// about mixing CLI config overrides back in.
    pub fn load_with_config<P>(path: P, config: ConfigSet) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        Self::build(path.into(), &[], Some(config))
    }

    fn build(
        path: PathBuf,
        pinned_config: &[PinnedConfig],
        config: Option<ConfigSet>,
    ) -> Result<Self> {
        let info = RepoMinimalInfo::from_repo_root(path)?;
        Self::build_with_info(info, pinned_config, config)
    }

    #[tracing::instrument(skip_all)]
    fn build_with_info(
        info: RepoMinimalInfo,
        pinned_config: &[PinnedConfig],
        config: Option<ConfigSet>,
    ) -> Result<Self> {
        constructors::init();

        assert!(
            config.is_none() || pinned_config.is_empty(),
            "Don't pass a config and CLI overrides to Repo::build"
        );

        let mut config = match config {
            Some(config) => config,
            None => configloader::hg::load(RepoInfo::Disk(&info), pinned_config)?,
        };

        mirror_requirement_to_config(&info, &mut config);

        let repo_name = configloader::hg::read_repo_name_from_disk(&info.shared_dot_hg_path)
            .ok()
            .or_else(|| {
                config
                    .get("remotefilelog", "reponame")
                    .map(|v| v.to_string())
            });

        let locker = Arc::new(RepoLocker::new(&config, info.store_path.clone())?);

        Ok(Repo {
            info,
            config: Arc::new(config),
            repo_name,
            metalog: Default::default(),
            eden_api: Default::default(),
            dag_commits: Default::default(),
            file_store: Default::default(),
            file_scm_store: Default::default(),
            tree_store: Default::default(),
            tree_scm_store: Default::default(),
            #[cfg(feature = "wdir")]
            working_copy: Default::default(),
            eager_store: None,
            tree_resolver: Default::default(),
            locker,
        })
    }

    pub fn lock(&self) -> Result<repolock::LockedPath, repolock::LockError> {
        self.locker.lock_store()
    }

    pub fn reload_requires(&mut self) -> Result<()> {
        let requirements = Requirements::load_repo_requirements(&self.dot_hg_path)?;
        let store_requirements = Requirements::load_store_requirements(&self.store_path)?;
        self.info.requirements = requirements;
        self.info.store_requirements = store_requirements;
        Ok(())
    }

    /// Invalidate all repo state.
    pub fn invalidate_all(&self) -> Result<()> {
        self.invalidate_dag_commits()?;
        self.invalidate_stores()?;
        self.invalidate_metalog()?;
        #[cfg(feature = "wdir")]
        self.invalidate_working_copy()?;
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

    pub fn config(&self) -> &Arc<dyn Config> {
        &self.config
    }

    pub fn set_config(&mut self, config: Arc<dyn Config>) {
        self.config = config;
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

    /// Identity used by the working copy.
    pub fn ident(&self) -> Identity {
        self.ident
    }

    #[cached_field]
    pub fn metalog(&self) -> Result<Arc<RwLock<MetaLog>>> {
        let metalog_path = self.metalog_path();
        let metalog = MetaLog::open_from_env(metalog_path.as_path())?;
        Ok(Arc::new(RwLock::new(metalog)))
    }

    pub fn metalog_path(&self) -> PathBuf {
        self.store_path.join("metalog")
    }

    /// Constructs the SaplingRemoteAPI client. Errors out if the SaplingRemoteAPI should not be
    /// constructed.
    ///
    /// Use `optional_eden_api` if `SaplingRemoteAPI` is optional.
    pub fn eden_api(&self) -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
        get_eden_api(self, &self.eden_api)
    }

    /// Constructs the SaplingRemoteAPI client. Errors out if the SaplingRemoteAPI should not be
    /// constructed or doesn't meet the required capabilities.
    ///
    /// Use `optional_eden_api_with_capabilities` if `SaplingRemoteAPI` is optional.
    pub fn eden_api_with_capabilities(
        &self,
        capabilities: HashSet<String>,
    ) -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
        get_eden_api_with_capabilities(self, &self.eden_api, capabilities)
    }

    /// Constructs SaplingRemoteAPI client if it should be constructed and has the basic sapling capabilities.
    ///
    /// Returns `None` if SaplingRemoteAPI should not be used or does not support the default capabilities.
    pub fn optional_eden_api(
        &self,
    ) -> Result<Option<Arc<dyn SaplingRemoteApi>>, SaplingRemoteApiError> {
        get_optional_eden_api(self, &self.eden_api)
    }

    #[cached_field]
    pub fn dag_commits(&self) -> Result<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>> {
        let info: &dyn StoreInfo = self;
        let commits: Box<dyn DagCommits + Send + 'static> = factory::call_constructor(info)?;
        Ok(Arc::new(RwLock::new(commits)))
    }

    pub fn remote_bookmarks(&self) -> Result<BTreeMap<RefName, HgId>> {
        self.metalog()?.read().get_remotenames()
    }

    pub fn set_remote_bookmarks(&self, names: &BTreeMap<RefName, HgId>) -> Result<()> {
        self.metalog()?.write().set_remotenames(names)
    }

    pub fn local_bookmarks(&self) -> Result<BTreeMap<RefName, HgId>> {
        self.metalog()?.read().get_bookmarks()
    }

    pub fn add_requirement(&mut self, requirement: &str) -> Result<()> {
        self.info.requirements.add(requirement);
        self.info.requirements.flush()?;
        Ok(())
    }

    pub fn add_store_requirement(&mut self, requirement: &str) -> Result<()> {
        self.info.store_requirements.add(requirement);
        self.info.store_requirements.flush()?;
        Ok(())
    }

    pub fn storage_format(&self) -> StorageFormat {
        let format = if self.requirements.contains("remotefilelog") {
            StorageFormat::RemoteFilelog
        } else if self.store_requirements.contains("git-store") {
            StorageFormat::Git
        } else if self.store_requirements.contains("eagerepo") {
            StorageFormat::Eagerepo
        } else {
            StorageFormat::Revlog
        };
        tracing::trace!("storage_format is {:?}", &format);
        format
    }

    pub fn file_store(&self) -> Result<Arc<dyn FileStore>> {
        if let Some(fs) = self.file_store.get() {
            return Ok(Arc::clone(fs));
        }

        if let Some((store, _)) = self.try_construct_file_tree_store()? {
            return Ok(store);
        }

        let fs = build_scm_file_store(self)?;
        let _ = self.file_scm_store.set(fs.clone());

        let fs = Arc::new(ArcFileStore(fs));

        let _ = self.file_store.set(fs.clone());
        tracing::trace!(target: "repo::file_store", "filestore created");

        Ok(fs)
    }

    // This should only be used to share stores with Python.
    pub fn file_scm_store(&self) -> Option<Arc<scmstore::FileStore>> {
        self.file_scm_store.get().cloned()
    }

    pub fn tree_store(&self) -> Result<Arc<dyn TreeStore>> {
        if let Some(ts) = self.tree_store.get() {
            return Ok(ts.clone());
        }

        if let Some((_, store)) = self.try_construct_file_tree_store()? {
            return Ok(store);
        }

        // Trigger construction of file store.
        let _ = self.file_store();

        let ts = build_scm_tree_store(self, self.file_scm_store())?;
        let _ = self.tree_scm_store.set(ts.clone());
        let _ = self.tree_store.set(ts.clone());

        Ok(ts)
    }

    // This should only be used to share stores with Python.
    pub fn tree_scm_store(&self) -> Option<Arc<scmstore::TreeStore>> {
        self.tree_scm_store.get().cloned()
    }

    // This should only be used to share stores with Python.
    pub fn eager_store(&self) -> Option<EagerRepoStore> {
        let store = self.file_store.get()?;
        let store = store.maybe_as_any()?.downcast_ref::<EagerRepoStore>()?;
        Some(store.clone())
    }

    pub fn tree_resolver(&self) -> Result<Arc<dyn ReadTreeManifest + Send + Sync>> {
        let tr = self.tree_resolver.get_or_try_init(|| {
            Ok::<_, anyhow::Error>(Arc::new(TreeManifestResolver::new(
                self.dag_commits()?,
                self.tree_store()?,
            )))
        })?;
        Ok(tr.clone())
    }

    pub fn resolve_commit(&self, treestate: Option<&TreeState>, change_id: &str) -> Result<HgId> {
        let dag = self.dag_commits()?;
        let dag = dag.read();
        let metalog = self.metalog()?;
        let metalog = metalog.read();
        let edenapi = self.optional_eden_api().map_err(|err| err.tag_network())?;
        revset_utils::resolve_single(
            self.config(),
            change_id,
            &dag.id_map_snapshot()?,
            &dag.dag_snapshot()?,
            &metalog,
            treestate,
            edenapi.as_deref(),
        )
    }

    pub fn resolve_commit_opt(
        &self,
        treestate: Option<&TreeState>,
        change_id: &str,
    ) -> Result<Option<HgId>> {
        match self.resolve_commit(treestate, change_id) {
            Ok(id) => Ok(Some(id)),
            Err(err) => match err.downcast_ref::<RevsetLookupError>() {
                Some(RevsetLookupError::RevsetNotFound(_)) => Ok(None),
                _ => Err(err),
            },
        }
    }

    pub fn invalidate_stores(&self) -> Result<()> {
        if let Some(file_store) = self.file_store.get() {
            file_store.refresh()?;
        }
        if let Some(tree_store) = self.tree_store.get() {
            tree_store.refresh()?;
        }
        Ok(())
    }

    /// Construct both file and tree store if they are backed by the same storage.
    /// Return None if they are not backed by the same storage.
    /// Return Some((file_store, tree_store)) if they are constructed.
    fn try_construct_file_tree_store(
        &self,
    ) -> Result<Option<(Arc<dyn FileStore>, Arc<dyn TreeStore>)>> {
        let info: &dyn StoreInfo = self;
        match factory::call_constructor::<_, Box<dyn StoreOutput>>(info) {
            Err(e) => {
                if factory::is_error_from_constructor(&e) {
                    Err(e)
                } else {
                    // Try other store constructors. Once revisionstore is migrated to
                    // use factory and abstraction, we can drop this.
                    Ok(None)
                }
            }
            Ok(out) => {
                let file_store = out.file_store();
                let tree_store = out.tree_store();
                let _ = self.file_store.set(file_store.clone());
                let _ = self.tree_store.set(tree_store.clone());
                Ok(Some((file_store, tree_store)))
            }
        }
    }
}

fn mirror_requirement_to_config(info: &RepoMinimalInfo, config: &mut ConfigSet) {
    // mirror "invalid-hash" requirement to "unsafe.skip-verify-hash" config
    // so code paths that can access configs but not requirements can still
    // learn the setting.
    config.set(
        "unsafe",
        "skip-verify-hash",
        Some(match info.store_requirements.contains("invalid-hash") {
            true => "true",
            false => "false",
        }),
        &"repo requirement".into(),
    );
}

#[cfg(feature = "wdir")]
impl Repo {
    #[cached_field]
    pub fn working_copy(&self) -> Result<Arc<RwLock<WorkingCopy>>> {
        tracing::trace!(target: "repo::workingcopy", "creating file store");
        let file_store = self.file_store()?;

        tracing::trace!(target: "repo::workingcopy", "creating tree resolver");
        let tree_resolver = self.tree_resolver()?;
        let has_requirement =
            |s: &str| self.requirements.contains(s) || self.store_requirements.contains(s);

        let wc = WorkingCopy::new(
            &self.path,
            self.config(),
            tree_resolver,
            file_store,
            self.locker.clone(),
            &self.dot_hg_path,
            &self.shared_dot_hg_path,
            &has_requirement,
        )
        .map_err(errors::InvalidWorkingCopy::from)?;

        Ok(Arc::new(RwLock::new(wc)))
    }
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("path", &self.path)
            .field("repo_name", &self.repo_name)
            .finish()
    }
}
