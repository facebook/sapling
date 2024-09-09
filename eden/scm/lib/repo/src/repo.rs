/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use cas_client::CasClient;
use commits_trait::DagCommits;
use configloader::config::ConfigSet;
use configloader::hg::PinnedConfig;
use configloader::Config;
use configmodel::ConfigExt;
use eagerepo::EagerRepo;
use eagerepo::EagerRepoStore;
use edenapi::Builder;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use manifest_tree::ReadTreeManifest;
use metalog::MetaLog;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use repo_minimal_info::constants::SUPPORTED_DEFAULT_REQUIREMENTS;
use repo_minimal_info::constants::SUPPORTED_STORE_REQUIREMENTS;
pub use repo_minimal_info::read_sharedpath;
use repo_minimal_info::RepoMinimalInfo;
use repo_minimal_info::Requirements;
use repolock::RepoLocker;
use repourl::RepoUrl;
use revisionstore::scmstore;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStoreBuilder;
use revisionstore::trait_impls::ArcFileStore;
use revisionstore::SaplingRemoteApiFileStore;
use revisionstore::SaplingRemoteApiTreeStore;
use revsets::errors::RevsetLookupError;
use revsets::utils as revset_utils;
use rewrite_macros::cached_field;
use storemodel::FileStore;
use storemodel::StoreInfo;
use storemodel::StoreOutput;
use storemodel::TreeStore;
use treestate::treestate::TreeState;
use types::repo::StorageFormat;
use types::HgId;
use util::path::absolute;
#[cfg(feature = "wdir")]
use workingcopy::workingcopy::WorkingCopy;

use crate::errors;
use crate::init;
use crate::trees::CachingTreeStore;
use crate::trees::TreeManifestResolver;

pub struct Repo {
    path: PathBuf,
    ident: identity::Identity,
    config: Arc<dyn Config>,
    shared_path: PathBuf,
    shared_ident: identity::Identity,
    pub(crate) store_path: PathBuf,
    dot_hg_path: PathBuf,
    shared_dot_hg_path: PathBuf,
    pub requirements: Requirements,
    pub store_requirements: Requirements,
    repo_name: Option<String>,
    metalog: OnceCell<Arc<RwLock<MetaLog>>>,
    eden_api: OnceCell<Arc<dyn SaplingRemoteApi>>,
    dag_commits: OnceCell<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>>,
    file_store: OnceCell<Arc<dyn FileStore>>,
    file_scm_store: OnceCell<Arc<scmstore::FileStore>>,
    tree_store: OnceCell<Arc<dyn TreeStore>>,
    tree_scm_store: OnceCell<Arc<scmstore::TreeStore>>,
    #[cfg(feature = "wdir")]
    working_copy: OnceCell<Arc<RwLock<WorkingCopy>>>,
    eager_store: Option<EagerRepoStore>,
    locker: Arc<RepoLocker>,
    cas_client: OnceCell<Option<Arc<dyn CasClient>>>,
    tree_resolver: OnceCell<Arc<dyn ReadTreeManifest + Send + Sync>>,
    caching_tree_store: OnceCell<Arc<dyn TreeStore>>,
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

        let config = match config {
            Some(config) => config,
            None => configloader::hg::load(Some(&info), pinned_config)?,
        };

        let RepoMinimalInfo {
            path,
            ident,
            shared_path,
            shared_ident,
            store_path,
            dot_hg_path,
            shared_dot_hg_path,
            requirements,
            store_requirements,
        } = info;

        let repo_name = configloader::hg::read_repo_name_from_disk(&shared_dot_hg_path)
            .ok()
            .or_else(|| {
                config
                    .get("remotefilelog", "reponame")
                    .map(|v| v.to_string())
            });

        let locker = Arc::new(RepoLocker::new(&config, store_path.clone())?);

        Ok(Repo {
            path,
            ident,
            config: Arc::new(config),
            shared_path,
            shared_ident,
            store_path,
            dot_hg_path,
            shared_dot_hg_path,
            requirements,
            store_requirements,
            repo_name,
            metalog: Default::default(),
            eden_api: Default::default(),
            cas_client: Default::default(),
            dag_commits: Default::default(),
            file_store: Default::default(),
            file_scm_store: Default::default(),
            tree_store: Default::default(),
            tree_scm_store: Default::default(),
            #[cfg(feature = "wdir")]
            working_copy: Default::default(),
            eager_store: None,
            tree_resolver: Default::default(),
            caching_tree_store: Default::default(),
            locker,
        })
    }

    pub fn lock(&self) -> Result<repolock::LockedPath, repolock::LockError> {
        self.locker.lock_store()
    }

    pub fn reload_requires(&mut self) -> Result<()> {
        self.requirements = Requirements::open(
            &self.dot_hg_path.join("requires"),
            &SUPPORTED_DEFAULT_REQUIREMENTS,
        )?;
        self.store_requirements = Requirements::open(
            &self.store_path.join("requires"),
            &SUPPORTED_STORE_REQUIREMENTS,
        )?;
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
        match self.optional_eden_api()? {
            Some(v) => Ok(v),
            None => Err(SaplingRemoteApiError::Other(anyhow!(
                "SaplingRemoteAPI is requested but not available for this repo"
            ))),
        }
    }

    /// Private API used by `optional_eden_api` that bypasses checks about whether
    /// SaplingRemoteAPI should be used or not.
    fn force_construct_eden_api(&self) -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
        let eden_api = self.eden_api.get_or_try_init(
            || -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
                tracing::trace!(target: "repo::eden_api", "creating edenapi");
                let eden_api = Builder::from_config(&self.config)?.build()?;
                tracing::info!(url=eden_api.url(), path=?self.path, "SaplingRemoteApi built");
                Ok(eden_api)
            },
        )?;
        Ok(eden_api.clone())
    }

    /// Constructs SaplingRemoteAPI client if it should be constructed.
    ///
    /// Returns `None` if SaplingRemoteAPI should not be used.
    pub fn optional_eden_api(
        &self,
    ) -> Result<Option<Arc<dyn SaplingRemoteApi>>, SaplingRemoteApiError> {
        if self.store_requirements.contains("git") {
            tracing::trace!(target: "repo::eden_api", "disabled because of git");
            return Ok(None);
        }
        if matches!(
            self.config.get_opt::<bool>("edenapi", "enable"),
            Ok(Some(false))
        ) {
            tracing::trace!(target: "repo::eden_api", "disabled because edenapi.enable is false");
            return Ok(None);
        }
        match self.config.get_nonempty_opt::<RepoUrl>("paths", "default") {
            Err(err) => {
                tracing::warn!(target: "repo::eden_api", ?err, "disabled because error parsing paths.default");
                return Ok(None);
            }
            Ok(None) => {
                tracing::trace!(target: "repo::eden_api", "disabled because paths.default is not set");
                return Ok(None);
            }
            Ok(Some(path)) => {
                // EagerRepo URLs (test:, eager: file path, dummyssh).
                if EagerRepo::url_to_dir(&path).is_some() {
                    tracing::trace!(target: "repo::eden_api", "using EagerRepo at {}", &path);
                    return Ok(Some(self.force_construct_eden_api()?));
                }
                // Legacy tests are incompatible with SaplingRemoteAPI.
                // They use None or file or ssh scheme with dummyssh.
                if path.scheme() == "file" {
                    tracing::trace!(target: "repo::eden_api", "disabled because paths.default is not set");
                    return Ok(None);
                } else if path.scheme() == "ssh" {
                    if let Some(ssh) = self.config.get("ui", "ssh") {
                        if ssh.contains("dummyssh") {
                            tracing::trace!(target: "repo::eden_api", "disabled because paths.default uses ssh scheme and dummyssh is in use");
                            return Ok(None);
                        }
                    }
                }
                // Explicitly set SaplingRemoteAPI URLs.
                // Ideally we can make paths.default derive the edenapi URLs. But "push" is not on
                // SaplingRemoteAPI yet. So we have to wait.
                if self.config.get_nonempty("edenapi", "url").is_none()
                    || self
                        .config
                        .get_nonempty("remotefilelog", "reponame")
                        .is_none()
                {
                    tracing::trace!(target: "repo::eden_api", "disabled because edenapi.url or remotefilelog.reponame is not set");
                    return Ok(None);
                }

                tracing::trace!(target: "repo::eden_api", "proceeding with path {}, reponame {:?}", path, self.config.get("remotefilelog", "reponame"));
            }
        }
        Ok(Some(self.force_construct_eden_api()?))
    }

    pub fn cas_client(&self) -> Result<Option<Arc<dyn CasClient>>> {
        Ok(self
            .cas_client
            .get_or_try_init(|| cas_client::new(self.config.clone()).context("building CasClient"))?
            .clone())
    }

    #[cached_field]
    pub fn dag_commits(&self) -> Result<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>> {
        let info: &dyn StoreInfo = self;
        let commits: Box<dyn DagCommits + Send + 'static> = factory::call_constructor(info)?;
        Ok(Arc::new(RwLock::new(commits)))
    }

    pub fn remote_bookmarks(&self) -> Result<BTreeMap<String, HgId>> {
        match self.metalog()?.read().get("remotenames")? {
            Some(rn) => Ok(refencode::decode_remotenames(&rn)?),
            None => Err(errors::RemotenamesMetalogKeyError.into()),
        }
    }

    pub fn set_remote_bookmarks(&self, names: &BTreeMap<String, HgId>) -> Result<()> {
        self.metalog()?
            .write()
            .set("remotenames", &refencode::encode_remotenames(names))?;
        Ok(())
    }

    pub fn local_bookmarks(&self) -> Result<BTreeMap<String, HgId>> {
        match self.metalog()?.read().get("bookmarks")? {
            Some(rn) => Ok(refencode::decode_bookmarks(&rn)?),
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

    pub fn storage_format(&self) -> StorageFormat {
        let format = if self.requirements.contains("remotefilelog") {
            StorageFormat::RemoteFilelog
        } else if self.store_requirements.contains("git") {
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

        tracing::trace!(target: "repo::file_store", "creating edenapi");
        let eden_api = self.optional_eden_api()?;

        tracing::trace!(target: "repo::file_store", "building filestore");
        let mut file_builder = FileStoreBuilder::new(self.config()).local_path(self.store_path());

        if let Some(eden_api) = eden_api {
            tracing::trace!(target: "repo::file_store", "enabling edenapi");
            file_builder = file_builder.edenapi(SaplingRemoteApiFileStore::new(eden_api));
        } else {
            tracing::trace!(target: "repo::file_store", "disabling edenapi");
            file_builder = file_builder.override_edenapi(false);
        }

        if let Some(cas_client) = self.cas_client()? {
            tracing::trace!(target: "repo::file_store", "setting cas client");
            file_builder = file_builder.cas_client(cas_client.clone());
        } else {
            tracing::trace!(target: "repo::file_store", "no cas client");
        }

        tracing::trace!(target: "repo::file_store", "building file store");
        let file_store = file_builder.build().context("when building FileStore")?;

        let fs = Arc::new(file_store);
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

        let eden_api = self.optional_eden_api()?;
        let mut tree_builder = TreeStoreBuilder::new(self.config())
            .local_path(self.store_path())
            .suffix("manifests");

        if let Some(eden_api) = eden_api {
            tracing::trace!(target: "repo::tree_store", "enabling edenapi");
            tree_builder = tree_builder.edenapi(SaplingRemoteApiTreeStore::new(eden_api));
        } else {
            tracing::trace!(target: "repo::tree_store", "disabling edenapi");
            tree_builder = tree_builder.override_edenapi(false);
        }

        if let Some(cas_client) = self.cas_client()? {
            tracing::trace!(target: "repo::tree_store", "setting cas client");
            tree_builder = tree_builder.cas_client(cas_client.clone());
        } else {
            tracing::trace!(target: "repo::tree_store", "no cas client");
        }

        // Trigger construction of file store.
        let _ = self.file_store();

        // The presence of the file store on the tree store causes the tree store to
        // request tree metadata (and write it back to file store aux cache).
        if let Some(file_store) = self.file_scm_store() {
            tracing::trace!(target: "repo::tree_store", "configuring filestore for aux fetching");
            tree_builder = tree_builder.filestore(file_store);
        } else {
            tracing::trace!(target: "repo::tree_store", "no filestore for aux fetching");
        }

        let ts = Arc::new(tree_builder.build()?);
        let _ = self.tree_scm_store.set(ts.clone());
        let _ = self.tree_store.set(ts.clone());
        Ok(ts)
    }

    pub fn caching_tree_store(&self) -> Result<Arc<dyn TreeStore>> {
        let store = self.caching_tree_store.get_or_try_init(|| {
            let cache_size = self
                .config
                // Trees are typically pretty small (and they are often kept in memory
                // anyway within a TreeManifest object), so let's pick a sizable
                // default. Set to 0 to disable caching.
                .get_or("experimental", "tree-resolver-cache-size", || 10_000)?;

            let inner_store = self.tree_store()?;
            if cache_size == 0 {
                return Ok::<_, anyhow::Error>(inner_store);
            }

            Ok(Arc::new(CachingTreeStore::new(inner_store, cache_size)))
        });
        Ok(store?.clone())
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
                self.caching_tree_store()?,
            )))
        })?;
        Ok(tr.clone())
    }

    pub fn resolve_commit(&self, treestate: Option<&TreeState>, change_id: &str) -> Result<HgId> {
        let dag = self.dag_commits()?;
        let dag = dag.read();
        let metalog = self.metalog()?;
        let metalog = metalog.read();
        let edenapi = self.optional_eden_api()?;
        revset_utils::resolve_single(
            &self.config,
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

#[cfg(feature = "wdir")]
impl Repo {
    #[cached_field]
    pub fn working_copy(&self) -> Result<Arc<RwLock<WorkingCopy>>> {
        tracing::trace!(target: "repo::workingcopy", "creating file store");
        let file_store = self.file_store()?;

        tracing::trace!(target: "repo::workingcopy", "creating tree resolver");
        let tree_resolver = self.tree_resolver()?;
        let has_requirement = |s: &str| self.requirements.contains(s);

        let wc = WorkingCopy::new(
            &self.path,
            &self.config,
            self.storage_format(),
            tree_resolver,
            file_store,
            self.locker.clone(),
            &self.dot_hg_path,
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
