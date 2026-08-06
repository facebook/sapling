/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_runtime::block_on;
use commits_trait::DagCommits;
use configmodel::Text;
use edenapi::SaplingRemoteApi;
use format_util::CommitFields;
use format_util::HgCommitLazyFields;
use format_util::hg_sha1_deserialize;
use manifest_tree::TreeManifest;
use manifest_tree::TreeResolver;
use manifest_tree::TreeStore;
use parking_lot::RwLock;
use types::HgId;
use types::hgid;

/// Error indicating that a tree was not found.
/// This is a specific error type that can be checked using `anyhow::Error::is::<TreeNotFoundError>()`.
/// Union resolvers use this to distinguish "not found" from unexpected errors.
#[derive(Debug, thiserror::Error)]
pub enum TreeNotFoundError {
    #[error("tree not found for commit {commit_id}")]
    Commit { commit_id: HgId },
    #[error("tree not found for root {root_id}")]
    Root { root_id: HgId },
}

/// A tree manifest resolver that fetches commit data from local disk.
pub struct LocalTreeResolver {
    dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
    tree_store: Arc<dyn TreeStore>,
}

impl LocalTreeResolver {
    pub fn new(
        dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
        tree_store: Arc<dyn TreeStore>,
    ) -> Self {
        LocalTreeResolver {
            dag_commits,
            tree_store,
        }
    }
}

impl TreeResolver for LocalTreeResolver {
    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        self.get_by_root_id(&self.get_root_id(commit_id)?)
    }

    fn get_by_root_id(&self, root_id: &HgId) -> Result<TreeManifest> {
        if root_id.is_null() {
            return Ok(TreeManifest::ephemeral(self.tree_store.clone()));
        }

        Ok(TreeManifest::durable(
            self.tree_store.clone(),
            root_id.clone(),
        ))
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        if commit_id.is_null() {
            // Special case: null commit's manifest node is null.
            return Ok(hgid::NULL_ID);
        }

        let commit_store = self.dag_commits.read().to_dyn_read_root_tree_ids();
        let tree_ids =
            async_runtime::block_on(commit_store.read_root_tree_ids(vec![commit_id.clone()]))?;

        if tree_ids.is_empty() {
            return Err(TreeNotFoundError::Commit {
                commit_id: commit_id.clone(),
            }
            .into());
        }

        Ok(tree_ids[0].1)
    }
}

/// A tree manifest resolver that fetches commit data from SLAPI.
pub struct SlapiTreeResolver {
    eden_api: Arc<dyn SaplingRemoteApi>,
    tree_store: Arc<dyn TreeStore>,
}

impl SlapiTreeResolver {
    pub fn new(eden_api: Arc<dyn SaplingRemoteApi>, tree_store: Arc<dyn TreeStore>) -> Self {
        SlapiTreeResolver {
            eden_api,
            tree_store,
        }
    }
}

impl TreeResolver for SlapiTreeResolver {
    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        self.get_by_root_id(&self.get_root_id(commit_id)?)
    }

    fn get_by_root_id(&self, root_id: &HgId) -> Result<TreeManifest> {
        if root_id.is_null() {
            return Ok(TreeManifest::ephemeral(self.tree_store.clone()));
        }

        Ok(TreeManifest::durable(
            self.tree_store.clone(),
            root_id.clone(),
        ))
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        if commit_id.is_null() {
            return Ok(hgid::NULL_ID);
        }

        let commit_text = block_on(async {
            self.eden_api
                .commit_revlog_data(vec![commit_id.clone()])
                .await?
                .single()
                .await
        })?;

        let text = commit_text
            .revlog_data
            .slice_to_bytes(hg_sha1_deserialize(&commit_text.revlog_data)?.0);
        let commit_fields = HgCommitLazyFields::new(Text::from_utf8_lossy(text));

        commit_fields.root_tree()
    }
}

/// A tree manifest resolver that tries multiple resolvers in order.
/// If a resolver returns a `TreeNotFoundError`, the next resolver is tried.
/// Other errors are propagated immediately.
pub struct UnionTreeResolver {
    resolvers: Vec<Arc<dyn TreeResolver + Send + Sync>>,
}

impl UnionTreeResolver {
    pub fn new(resolvers: Vec<Arc<dyn TreeResolver + Send + Sync>>) -> Self {
        UnionTreeResolver { resolvers }
    }
}

impl TreeResolver for UnionTreeResolver {
    fn get_by_root_id(&self, root_id: &HgId) -> Result<TreeManifest> {
        let mut last_err = None;
        for resolver in &self.resolvers {
            match resolver.get_by_root_id(root_id) {
                Ok(manifest) => return Ok(manifest),
                Err(e) if e.is::<TreeNotFoundError>() => {
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            TreeNotFoundError::Root {
                root_id: root_id.clone(),
            }
            .into()
        }))
    }

    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        match self.get_by_root_id(&self.get_root_id(commit_id)?) {
            Ok(manifest) => Ok(manifest),
            Err(e) if e.is::<TreeNotFoundError>() => Err(TreeNotFoundError::Commit {
                commit_id: commit_id.clone(),
            }
            .into()),
            Err(e) => Err(e),
        }
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        let mut last_err = None;
        for resolver in &self.resolvers {
            match resolver.get_root_id(commit_id) {
                Ok(id) => return Ok(id),
                Err(e) if e.is::<TreeNotFoundError>() => {
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            TreeNotFoundError::Commit {
                commit_id: commit_id.clone(),
            }
            .into()
        }))
    }
}

/// A resolver wrapper which synthesizes virtual TreeManifest based off
/// the commit content. Useful for trees which are not directly available
/// in store but can be derived from the commit content (e.g. projects in .repo/manifests).
pub struct GrepoTreeResolver {
    inner_resolver: Arc<dyn TreeResolver>,
    synthesize_fn: Arc<dyn Fn(&TreeManifest) -> Result<TreeManifest> + Send + Sync>,
}

impl GrepoTreeResolver {
    pub fn new(
        inner_resolver: Arc<dyn TreeResolver>,
        synthesize_fn: Arc<dyn Fn(&TreeManifest) -> Result<TreeManifest> + Send + Sync>,
    ) -> Self {
        GrepoTreeResolver {
            inner_resolver,
            synthesize_fn,
        }
    }
}

impl TreeResolver for GrepoTreeResolver {
    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        self.get_by_root_id(&self.get_root_id(commit_id)?)
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        self.inner_resolver.get_root_id(commit_id)
    }

    fn get_by_root_id(&self, root_id: &HgId) -> Result<TreeManifest> {
        let manifest = self.inner_resolver.get_by_root_id(root_id)?;
        if !root_id.is_null() {
            return (self.synthesize_fn)(&manifest)
                .with_context(|| format!("synthesizing tree for root {}", root_id.to_hex()));
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock resolver that returns not found for any commit.
    struct NotFoundResolver;

    impl TreeResolver for NotFoundResolver {
        fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
            Err(TreeNotFoundError::Commit {
                commit_id: commit_id.clone(),
            }
            .into())
        }

        fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
            Err(TreeNotFoundError::Commit {
                commit_id: commit_id.clone(),
            }
            .into())
        }

        fn get_by_root_id(&self, root_id: &HgId) -> Result<TreeManifest> {
            Err(TreeNotFoundError::Root {
                root_id: root_id.clone(),
            }
            .into())
        }
    }

    /// A mock resolver that returns a specific root ID.
    struct SuccessResolver {
        root_id: HgId,
    }

    impl TreeResolver for SuccessResolver {
        fn get(&self, _commit_id: &HgId) -> Result<TreeManifest> {
            // For testing, we only care about get_root_id
            panic!("get() not implemented for SuccessResolver in tests")
        }

        fn get_root_id(&self, _commit_id: &HgId) -> Result<HgId> {
            Ok(self.root_id.clone())
        }

        fn get_by_root_id(&self, _root_id: &HgId) -> Result<TreeManifest> {
            panic!("get_by_root_id() not implemented for SuccessResolver in tests")
        }
    }

    /// A mock resolver that returns an unexpected error.
    struct ErrorResolver {
        message: String,
    }

    impl TreeResolver for ErrorResolver {
        fn get(&self, _commit_id: &HgId) -> Result<TreeManifest> {
            Err(anyhow::anyhow!("{}", self.message))
        }

        fn get_root_id(&self, _commit_id: &HgId) -> Result<HgId> {
            Err(anyhow::anyhow!("{}", self.message))
        }

        fn get_by_root_id(&self, _root_id: &HgId) -> Result<TreeManifest> {
            Err(anyhow::anyhow!("{}", self.message))
        }
    }

    #[test]
    fn test_union_resolver_first_succeeds() {
        let resolver = UnionTreeResolver::new(vec![
            Arc::new(SuccessResolver {
                root_id: HgId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
            }),
            Arc::new(NotFoundResolver),
        ]);

        let commit_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let result = resolver.get_root_id(&commit_id).unwrap();
        assert_eq!(
            result,
            HgId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
        );
    }

    #[test]
    fn test_union_resolver_fallback_on_not_found() {
        let resolver = UnionTreeResolver::new(vec![
            Arc::new(NotFoundResolver),
            Arc::new(SuccessResolver {
                root_id: HgId::from_hex(b"cccccccccccccccccccccccccccccccccccccccc").unwrap(),
            }),
        ]);

        let commit_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let result = resolver.get_root_id(&commit_id).unwrap();
        assert_eq!(
            result,
            HgId::from_hex(b"cccccccccccccccccccccccccccccccccccccccc").unwrap()
        );
    }

    #[test]
    fn test_union_resolver_propagates_unexpected_error() {
        let resolver = UnionTreeResolver::new(vec![
            Arc::new(ErrorResolver {
                message: "unexpected error".to_string(),
            }),
            Arc::new(SuccessResolver {
                root_id: HgId::from_hex(b"cccccccccccccccccccccccccccccccccccccccc").unwrap(),
            }),
        ]);

        let commit_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let err = resolver.get_root_id(&commit_id).unwrap_err();
        assert!(!err.is::<TreeNotFoundError>());
        assert!(err.to_string().contains("unexpected error"));
    }

    #[test]
    fn test_union_resolver_all_not_found() {
        let resolver =
            UnionTreeResolver::new(vec![Arc::new(NotFoundResolver), Arc::new(NotFoundResolver)]);

        let commit_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let err = resolver.get_root_id(&commit_id).unwrap_err();
        assert!(err.is::<TreeNotFoundError>());
    }

    #[test]
    fn test_union_resolver_empty() {
        let resolver = UnionTreeResolver::new(vec![]);

        let commit_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let err = resolver.get_root_id(&commit_id).unwrap_err();
        assert!(err.is::<TreeNotFoundError>());
    }

    #[test]
    fn test_tree_not_found_error_display() {
        let commit_id = HgId::from_hex(b"dddddddddddddddddddddddddddddddddddddddd").unwrap();
        let err = TreeNotFoundError::Commit { commit_id };
        assert!(
            err.to_string()
                .contains("dddddddddddddddddddddddddddddddddddddddd")
        );
        assert!(err.to_string().contains("tree not found"));
    }
}
