/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use configloader::Config;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use pathmatcher::DynMatcher;
use storemodel::FileStore;
use storemodel::TreeStore;
use types::HgId;

use crate::repo::Repo;
use crate::slapi_repo::SlapiRepo;

/// An enum that abstracts over Repo and SlapiRepo, providing a common interface
/// for operations that work with either type.
pub enum CoreRepo {
    /// A repository with local disk presence.
    Disk(Repo),
    /// A lightweight repository without local disk presence.
    Slapi(SlapiRepo),
}

impl CoreRepo {
    pub fn repo_name(&self) -> Option<&str> {
        match self {
            CoreRepo::Disk(repo) => repo.repo_name(),
            CoreRepo::Slapi(repo) => Some(repo.repo_name()),
        }
    }

    /// Get the config.
    pub fn config(&self) -> &Arc<dyn Config> {
        match self {
            CoreRepo::Disk(repo) => repo.config(),
            CoreRepo::Slapi(repo) => repo.config(),
        }
    }

    /// Set the config.
    pub fn set_config(&mut self, config: Arc<dyn Config>) {
        match self {
            CoreRepo::Disk(repo) => repo.set_config(config),
            CoreRepo::Slapi(repo) => repo.set_config(config),
        }
    }

    /// Get the tree resolver.
    pub fn tree_resolver(&self) -> Result<Arc<dyn ReadTreeManifest + Send + Sync>> {
        match self {
            CoreRepo::Disk(repo) => repo.tree_resolver(),
            CoreRepo::Slapi(repo) => repo.tree_resolver(),
        }
    }

    /// Get the tree store.
    pub fn tree_store(&self) -> Result<Arc<dyn TreeStore>> {
        match self {
            CoreRepo::Disk(repo) => repo.tree_store(),
            CoreRepo::Slapi(repo) => repo.tree_store(),
        }
    }

    /// Get the file store.
    pub fn file_store(&self) -> Result<Arc<dyn FileStore>> {
        match self {
            CoreRepo::Disk(repo) => repo.file_store(),
            CoreRepo::Slapi(repo) => repo.file_store(),
        }
    }

    /// Resolve a commit identifier to an HgId.
    /// Supports various formats like hex commit hash prefixes, bookmark names, etc.
    ///
    /// For `Repo`, this uses the working copy's treestate for resolving "." and similar.
    /// For `SlapiRepo`, this only supports remote lookups (hash prefixes and bookmarks).
    pub fn resolve_commit(&self, change_id: &str) -> Result<HgId> {
        match self {
            CoreRepo::Disk(repo) => {
                #[cfg(feature = "wdir")]
                {
                    let wc = repo.working_copy()?;
                    let wc = wc.read();
                    let treestate = wc.treestate();
                    let treestate = treestate.lock();
                    repo.resolve_commit(Some(&treestate), change_id)
                }
                #[cfg(not(feature = "wdir"))]
                {
                    repo.resolve_commit(None, change_id)
                }
            }
            CoreRepo::Slapi(repo) => repo.resolve_commit(change_id),
        }
    }

    /// Get the sparse matcher if sparse is enabled.
    ///
    /// For `Repo`, this delegates to the working copy's sparse_matcher.
    /// For `SlapiRepo`, this checks for a code-tenting sparse profile in the config
    /// (clone.additional-sparse-profile) and builds a matcher from it.
    pub fn sparse_matcher(&self, manifest: &TreeManifest) -> Result<Option<DynMatcher>> {
        match self {
            #[cfg(feature = "wdir")]
            CoreRepo::Disk(repo) => {
                let wc = repo.working_copy()?;
                let wc = wc.read();
                wc.sparse_matcher()
            }
            #[cfg(not(feature = "wdir"))]
            CoreRepo::Disk(_) => Ok(None),
            CoreRepo::Slapi(repo) => repo.sparse_matcher(manifest),
        }
    }
}

impl From<Repo> for CoreRepo {
    fn from(repo: Repo) -> Self {
        CoreRepo::Disk(repo)
    }
}

impl From<SlapiRepo> for CoreRepo {
    fn from(repo: SlapiRepo) -> Self {
        CoreRepo::Slapi(repo)
    }
}

impl std::fmt::Debug for CoreRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreRepo::Disk(repo) => f.debug_tuple("CoreRepo::Disk").field(repo).finish(),
            CoreRepo::Slapi(repo) => f.debug_tuple("CoreRepo::Slapi").field(repo).finish(),
        }
    }
}
