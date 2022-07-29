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

use anyhow::Result;
use configparser::config::ConfigSet;
use configparser::Config;
use edenapi::Builder;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use hgcommits::DagCommits;
use metalog::MetaLog;
use parking_lot::RwLock;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStoreBuilder;
use revisionstore::trait_impls::ArcFileStore;
use revisionstore::EdenApiFileStore;
use revisionstore::EdenApiTreeStore;
use revisionstore::MemcacheStore;
use storemodel::ReadFileContents;
use storemodel::TreeStore;
use types::HgId;
use util::path::absolute;

use crate::commits::open_dag_commits;
use crate::errors;
use crate::init;
use crate::requirements::Requirements;

pub struct Repo {
    path: PathBuf,
    config: ConfigSet,
    shared_path: PathBuf,
    store_path: PathBuf,
    dot_hg_path: PathBuf,
    shared_dot_hg_path: PathBuf,
    pub requirements: Requirements,
    pub store_requirements: Requirements,
    repo_name: Option<String>,
    metalog: Option<Arc<RwLock<MetaLog>>>,
    eden_api: Option<Arc<dyn EdenApi>>,
    dag_commits: Option<Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>>,
    file_store: Option<Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>>,
    tree_store: Option<Arc<dyn TreeStore + Send + Sync>>,
}

impl Repo {
    pub fn init(
        root_path: &Path,
        config: &ConfigSet,
        hgrc_contents: Option<String>,
        extra_config_values: &[String],
    ) -> Result<Repo> {
        let root_path = absolute(root_path)?;
        init::init_hg_repo(&root_path, config, hgrc_contents)?;
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
        let path = path.into();
        assert!(path.is_absolute());

        let dot_hg_path = path.join(".hg");
        let config =
            configparser::hg::load(Some(&dot_hg_path), extra_config_values, extra_config_files)?;
        Self::load_with_config(path, config)
    }

    /// Loads the repo at given path, eschewing any config loading in
    /// favor of given config. This method exists so Python can create
    /// a Repo that uses the Python config verbatim without worrying
    /// about mixing CLI config overrides back in.
    pub fn load_with_config<P>(path: P, config: ConfigSet) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        assert!(path.is_absolute());

        let shared_path = read_sharedpath(&path)?;
        let dot_hg_path = path.join(".hg");
        let shared_dot_hg_path = shared_path.join(".hg");
        let store_path = shared_dot_hg_path.join("store");

        let repo_name = configparser::hg::read_repo_name_from_disk(&shared_dot_hg_path)
            .ok()
            .or_else(|| {
                config
                    .get("remotefilelog", "reponame")
                    .map(|v| v.to_string())
            });

        let requirements = Requirements::open(&dot_hg_path.join("requires"))?;
        let store_requirements = Requirements::open(&store_path.join("requires"))?;

        Ok(Repo {
            path,
            config,
            shared_path,
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
        })
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

    pub fn repo_name(&self) -> Option<&str> {
        self.repo_name.as_ref().map(|s| s.as_ref())
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
                let correlator = edenapi::DEFAULT_CORRELATOR.as_str();
                let eden_api = Builder::from_config(&self.config)?
                    .correlator(Some(correlator))
                    .build()?;
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

    pub fn invalidate_dag_commits(&mut self) {
        self.dag_commits = None;
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
            return Ok(fs.clone());
        }

        let eden_api = self.eden_api()?;
        let mut file_builder = FileStoreBuilder::new(self.config())
            .edenapi(EdenApiFileStore::new(eden_api))
            .local_path(self.store_path())
            .correlator(edenapi::DEFAULT_CORRELATOR.as_str());

        if self.config.get_or_default("scmstore", "auxindexedlog")? {
            file_builder = file_builder.store_aux_data();
        }

        if self
            .config
            .get_nonempty("remotefilelog", "cachekey")
            .is_some()
        {
            file_builder = file_builder.memcache(Arc::new(MemcacheStore::new(&self.config)?));
        }

        let fs = Arc::new(ArcFileStore(Arc::new(file_builder.build()?)));

        self.file_store = Some(fs.clone());

        Ok(fs)
    }

    pub fn tree_store(&mut self) -> Result<Arc<dyn TreeStore + Send + Sync>> {
        if let Some(ts) = &self.tree_store {
            return Ok(ts.clone());
        }

        let eden_api = self.eden_api()?;
        let tree_builder = TreeStoreBuilder::new(self.config())
            .edenapi(EdenApiTreeStore::new(eden_api))
            .local_path(self.store_path())
            .suffix("manifests");
        let ts = Arc::new(tree_builder.build()?);
        self.tree_store = Some(ts.clone());
        Ok(ts)
    }
}

fn read_sharedpath(path: &Path) -> Result<PathBuf> {
    let mut sharedpath = fs::read_to_string(path.join(".hg/sharedpath"))
        .ok()
        .map(|s| PathBuf::from(s))
        .and_then(|p| Some(PathBuf::from(p.parent()?)));

    if let Some(possible_path) = sharedpath {
        if possible_path.is_absolute() && !possible_path.is_dir() {
            return Err(errors::InvalidSharedPath(
                possible_path.join(".hg").to_string_lossy().to_string(),
            )
            .into());
        } else if possible_path.is_absolute() {
            sharedpath = Some(possible_path)
        } else {
            // join relative path from the REPO/.hg path
            let new_possible = path.join(".hg").join(possible_path);
            if !new_possible.join(".hg").exists() {
                return Err(
                    errors::InvalidSharedPath(new_possible.to_string_lossy().to_string()).into(),
                );
            }
            sharedpath = Some(new_possible)
        }
    }
    Ok(sharedpath.unwrap_or_else(|| path.to_path_buf()))
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("path", &self.path)
            .field("repo_name", &self.repo_name)
            .finish()
    }
}
