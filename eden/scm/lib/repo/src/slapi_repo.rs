/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use async_runtime::block_on;
use configloader::Config;
use configloader::config::Options;
use configloader::hg::RepoInfo;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use metalog::MetaLog;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use pathmatcher::DynMatcher;
use repourl::RepoUrl;
use revisionstore::trait_impls::ArcFileStore;
use revsets::errors::RevsetLookupError;
use revsets::utils::remote_hash_prefix_lookup;
use sparse::Root;
use storemodel::FileStore;
use storemodel::StoreInfo;
use storemodel::TreeStore;
use types::HgId;
use workingcopy::sparse::build_matcher;

use crate::scmstore::build_scm_file_store;
use crate::scmstore::build_scm_tree_store;
use crate::slapi_client::OnceSlapi;
use crate::slapi_client::get_eden_api;
use crate::trees::SlapiTreeResolver;

/// A lightweight repo flavor that doesn't require local disk presence.
/// It can load config from a RepoUrl and provides access to SLAPI client
/// and SCM stores.
pub struct SlapiRepo {
    config: Arc<dyn Config>,
    repo_name: String,
    eden_api: OnceSlapi,
    file_store: OnceCell<Arc<dyn FileStore>>,
    tree_store: OnceCell<Arc<dyn TreeStore>>,
    tree_resolver: OnceCell<Arc<dyn ReadTreeManifest + Send + Sync>>,
}

impl SlapiRepo {
    /// Load a SlapiRepo from a RepoUrl.
    /// Uses RepoInfo::Ephemeral to load config without requiring local disk presence.
    pub fn load(url: &RepoUrl) -> Result<Self> {
        constructors::init();

        let repo_name = url
            .repo_name()
            .ok_or_else(|| anyhow!("RepoUrl must have a repo name"))?
            .to_string();

        let mut config = configloader::hg::load(RepoInfo::Ephemeral(&repo_name), &[])?;

        // Set paths.default and remotefilelog.reponame so SLAPI client construction works.
        let opts = Options::new().source("SlapiRepo::load");
        config.set("paths", "default", Some(url.clean_str()), &opts);
        config.set("remotefilelog", "reponame", Some(&repo_name), &opts);

        Ok(Self {
            config: Arc::new(config),
            repo_name,
            eden_api: Default::default(),
            file_store: Default::default(),
            tree_store: Default::default(),
            tree_resolver: Default::default(),
        })
    }

    pub fn config(&self) -> &Arc<dyn Config> {
        &self.config
    }

    pub fn set_config(&mut self, config: Arc<dyn Config>) {
        self.config = config;
    }

    pub fn repo_name(&self) -> &str {
        &self.repo_name
    }

    /// Get the SLAPI client, constructing it if necessary.
    pub fn eden_api(&self) -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
        get_eden_api(self, &self.eden_api)
    }

    /// Get the file store, constructing it if necessary.
    pub fn file_store(&self) -> Result<Arc<dyn FileStore>> {
        if let Some(fs) = self.file_store.get() {
            return Ok(Arc::clone(fs));
        }

        let fs = build_scm_file_store(self)?;
        let fs: Arc<dyn FileStore> = Arc::new(ArcFileStore(fs));
        let _ = self.file_store.set(fs.clone());

        Ok(fs)
    }

    /// Get the tree store, constructing it if necessary.
    pub fn tree_store(&self) -> Result<Arc<dyn TreeStore>> {
        if let Some(ts) = self.tree_store.get() {
            return Ok(Arc::clone(ts));
        }

        let fs = self.file_store.get().and_then(|fs| {
            fs.maybe_as_any()?
                .downcast_ref::<ArcFileStore>()
                .map(|fs| fs.0.clone())
        });
        let ts = build_scm_tree_store(self, fs)?;
        let ts: Arc<dyn TreeStore> = ts;
        let _ = self.tree_store.set(ts.clone());

        Ok(ts)
    }

    /// Get the tree resolver, constructing it if necessary.
    pub fn tree_resolver(&self) -> Result<Arc<dyn ReadTreeManifest + Send + Sync>> {
        let tr = self.tree_resolver.get_or_try_init(|| {
            Ok::<_, anyhow::Error>(Arc::new(SlapiTreeResolver::new(
                self.eden_api()?,
                self.tree_store()?,
            )))
        })?;
        Ok(Arc::clone(tr))
    }

    /// Resolve a commit identifier to an HgId.
    /// Supports hex commit hash prefixes and bookmark names.
    pub fn resolve_commit(&self, id: &str) -> Result<HgId> {
        let slapi = self.eden_api()?;

        // Check if this looks like a hex commit hash prefix.
        if id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            if !id.is_empty() && id.len() <= 40 {
                if let Some(hgid) = remote_hash_prefix_lookup(slapi.as_ref(), id)? {
                    return Ok(hgid);
                }
            }
        }

        // Fall back to bookmark lookup.
        let mut bms = block_on(slapi.bookmarks(vec![id.to_string()], None))?;

        match bms.pop().and_then(|bm| bm.hgid) {
            None => Err(RevsetLookupError::RevsetNotFound(id.to_owned()).into()),
            Some(hgid) => Ok(hgid),
        }
    }

    /// Get sparse matcher for code tenting.
    ///
    /// Checks for a code-tenting sparse profile in the config and builds a matcher from
    /// it if present.
    pub fn sparse_matcher(&self, manifest: &TreeManifest) -> Result<Option<DynMatcher>> {
        match filters::util::filter_paths_from_config(self.config()) {
            // {""} is a special case that means "null filter" - match eveything.
            Some(paths) if !paths.iter().all(|p| p.is_empty()) => {
                let sparse_root = Root::from_bytes(
                    paths
                        .into_iter()
                        .map(|p| format!("%include {p}\n"))
                        .collect::<Vec<_>>()
                        .join(""),
                    "SlapiRepo".to_string(),
                )?;
                let (sparse_matcher, _) =
                    build_matcher(&sparse_root, manifest, self.file_store()?, &HashMap::new())?;

                Ok(Some(Arc::new(sparse_matcher)))
            }
            _ => Ok(None),
        }
    }
}

impl StoreInfo for SlapiRepo {
    fn has_requirement(&self, requirement: &str) -> bool {
        matches!(requirement, "remotefilelog")
    }

    fn config(&self) -> &dyn configmodel::Config {
        self.config.as_ref()
    }

    fn store_path(&self) -> Option<&Path> {
        None
    }

    fn remote_peer(&self) -> anyhow::Result<Option<Arc<dyn SaplingRemoteApi>>> {
        Ok(Some(self.eden_api()?))
    }

    fn metalog(&self) -> anyhow::Result<Arc<RwLock<MetaLog>>> {
        Err(anyhow!("SlapiRepo does not have a metalog"))
    }
}

impl std::fmt::Debug for SlapiRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlapiRepo")
            .field("repo_name", &self.repo_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use async_runtime::block_on;
    use eagerepo::EagerRepo;
    use format_util::hg_sha1_serialize;
    use manifest_tree::Flag;
    use manifest_tree::Manifest;
    use manifest_tree::TreeElement;
    use manifest_tree::TreeEntry;
    use pathmatcher::AlwaysMatcher;
    use storemodel::SerializationFormat;
    use types::FetchContext;
    use types::HgId;
    use types::Key;
    use types::PathComponentBuf;
    use types::RepoPathBuf;
    use types::fetch_mode::FetchMode;

    use super::*;

    #[test]
    fn test_eden_api_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let eager_repo = EagerRepo::open(dir.path()).unwrap();
        block_on(eager_repo.flush()).unwrap();

        let config = BTreeMap::<&str, &str>::new();
        let url = RepoUrl::from_str(&config, &format!("eager:{}", dir.path().display())).unwrap();
        let slapi_repo = SlapiRepo::load(&url).unwrap();

        let eden_api = slapi_repo.eden_api().unwrap();
        let caps = block_on(eden_api.capabilities()).unwrap();
        assert!(caps.contains(&"sapling-common".to_string()));
    }

    #[test]
    fn test_fetch_file() {
        let dir = tempfile::tempdir().unwrap();
        let eager_repo = EagerRepo::open(dir.path()).unwrap();

        let file_content = b"hello world";
        let file_data = hg_sha1_serialize(file_content, HgId::null_id(), HgId::null_id());
        let file_id = eager_repo.add_sha1_blob(&file_data).unwrap();
        block_on(eager_repo.flush()).unwrap();

        let config = BTreeMap::<&str, &str>::new();
        let url = RepoUrl::from_str(&config, &format!("eager:{}", dir.path().display())).unwrap();
        let slapi_repo = SlapiRepo::load(&url).unwrap();

        let file_store = slapi_repo.file_store().unwrap();
        let key = Key::new(
            RepoPathBuf::from_string("test.txt".to_string()).unwrap(),
            file_id,
        );
        let fctx = FetchContext::new(FetchMode::AllowRemote);
        let content = file_store.get_content(fctx, &key.path, key.hgid).unwrap();
        assert_eq!(content.into_vec(), file_content);
    }

    #[test]
    fn test_fetch_tree() {
        let dir = tempfile::tempdir().unwrap();
        let eager_repo = EagerRepo::open(dir.path()).unwrap();

        let elements = vec![TreeElement::new(
            PathComponentBuf::from_string("foo.txt".to_string()).unwrap(),
            HgId::null_id().clone(),
            Flag::File(Default::default()),
        )];
        let entry = TreeEntry::from_elements(elements, SerializationFormat::Hg);
        let tree_content = entry.as_ref();
        let tree_data = hg_sha1_serialize(tree_content, HgId::null_id(), HgId::null_id());
        let tree_id = eager_repo.add_sha1_blob(&tree_data).unwrap();
        block_on(eager_repo.flush()).unwrap();

        let config = BTreeMap::<&str, &str>::new();
        let url = RepoUrl::from_str(&config, &format!("eager:{}", dir.path().display())).unwrap();
        let slapi_repo = SlapiRepo::load(&url).unwrap();

        let tree_store = slapi_repo.tree_store().unwrap();
        let key = Key::new(RepoPathBuf::from_string("".to_string()).unwrap(), tree_id);
        let fctx = FetchContext::new(FetchMode::AllowRemote);
        let content = tree_store.get_content(fctx, &key.path, key.hgid).unwrap();
        assert_eq!(content.into_vec(), tree_content);
    }

    #[test]
    fn test_tree_resolver() {
        let dir = tempfile::tempdir().unwrap();
        let eager_repo = EagerRepo::open(dir.path()).unwrap();

        // Create a tree with one file.
        let elements = vec![TreeElement::new(
            PathComponentBuf::from_string("bar.txt".to_string()).unwrap(),
            HgId::null_id().clone(),
            Flag::File(Default::default()),
        )];
        let entry = TreeEntry::from_elements(elements, SerializationFormat::Hg);
        let tree_content = entry.as_ref();
        let tree_data = hg_sha1_serialize(tree_content, HgId::null_id(), HgId::null_id());
        let tree_id = eager_repo.add_sha1_blob(&tree_data).unwrap();

        // Create a commit referencing the tree.
        let commit_text = format!("{}\ntest\n\ntest commit", tree_id.to_hex());
        let commit_data =
            hg_sha1_serialize(commit_text.as_bytes(), HgId::null_id(), HgId::null_id());
        let commit_id = eager_repo.add_sha1_blob(&commit_data).unwrap();
        block_on(eager_repo.flush()).unwrap();

        let config = BTreeMap::<&str, &str>::new();
        let url = RepoUrl::from_str(&config, &format!("eager:{}", dir.path().display())).unwrap();
        let slapi_repo = SlapiRepo::load(&url).unwrap();

        // Use tree_resolver to get root tree id from commit.
        let tree_resolver = slapi_repo.tree_resolver().unwrap();
        let root_id = tree_resolver.get_root_id(&commit_id).unwrap();
        assert_eq!(root_id, tree_id);

        // Use tree_resolver to get tree manifest from commit.
        let manifest = tree_resolver.get(&commit_id).unwrap();
        let files: Vec<_> = manifest.files(AlwaysMatcher::new()).collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].as_ref().unwrap().path.as_str(), "bar.txt");
    }

    #[test]
    fn test_resolve_commit() {
        let dir = tempfile::tempdir().unwrap();
        let eager_repo = EagerRepo::open(dir.path()).unwrap();

        // Create a tree with one file.
        let elements = vec![TreeElement::new(
            PathComponentBuf::from_string("file.txt".to_string()).unwrap(),
            HgId::null_id().clone(),
            Flag::File(Default::default()),
        )];
        let entry = TreeEntry::from_elements(elements, SerializationFormat::Hg);
        let tree_content = entry.as_ref();
        let tree_data = hg_sha1_serialize(tree_content, HgId::null_id(), HgId::null_id());
        let tree_id = eager_repo.add_sha1_blob(&tree_data).unwrap();

        // Create a commit referencing the tree using add_commit to properly register in DAG.
        let commit_text = format!("{}\ntest\n\ntest commit", tree_id.to_hex());
        let commit_id = block_on(eager_repo.add_commit(&[], commit_text.as_bytes())).unwrap();

        // Set a bookmark pointing to the commit.
        eager_repo.set_bookmark("main", Some(commit_id)).unwrap();

        block_on(eager_repo.flush()).unwrap();

        let config = BTreeMap::<&str, &str>::new();
        let url = RepoUrl::from_str(&config, &format!("eager:{}", dir.path().display())).unwrap();
        let slapi_repo = SlapiRepo::load(&url).unwrap();

        // Test 1: Resolve by full commit hash.
        let resolved = slapi_repo.resolve_commit(&commit_id.to_hex()).unwrap();
        assert_eq!(resolved, commit_id);

        // Test 2: Resolve by hash prefix.
        let prefix = &commit_id.to_hex()[..12];
        let resolved = slapi_repo.resolve_commit(prefix).unwrap();
        assert_eq!(resolved, commit_id);

        // Test 3: Resolve by bookmark name.
        let resolved = slapi_repo.resolve_commit("main").unwrap();
        assert_eq!(resolved, commit_id);

        // Test 4: Not found case.
        let err = slapi_repo.resolve_commit("nonexistent").unwrap_err();
        assert!(err.downcast_ref::<RevsetLookupError>().is_some());
    }
}
