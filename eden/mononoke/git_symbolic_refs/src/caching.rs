/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::GitSymbolicRefs;
use crate::GitSymbolicRefsEntry;
use crate::RefType;

/// A caching wrapper around a GitSymbolicRefs implementation. Note that this caches ALL known
/// symrefs for a given repo at the time of its creation. This means that using this wrapper will
/// avoid all calls to DB but will also mean that any symrefs added or deleted after the wrapper
/// is created will not be reflected in the cache.
#[derive(Clone)]
pub struct CachedGitSymbolicRefs {
    git_symbolic_refs: Arc<dyn GitSymbolicRefs>,
    cached_entries: HashSet<GitSymbolicRefsEntry>,
}

impl CachedGitSymbolicRefs {
    pub async fn new(
        ctx: &CoreContext,
        git_symbolic_refs: Arc<dyn GitSymbolicRefs>,
    ) -> Result<Self> {
        let cached_entries = git_symbolic_refs
            .list_all_symrefs(ctx)
            .await?
            .into_iter()
            .collect();
        Ok(Self {
            git_symbolic_refs,
            cached_entries,
        })
    }
}

#[async_trait]
impl GitSymbolicRefs for CachedGitSymbolicRefs {
    /// The repository for which these symrefs exist
    fn repo_id(&self) -> RepositoryId {
        self.git_symbolic_refs.repo_id()
    }

    /// Fetch the symbolic ref entry corresponding to the symref name in the
    /// given repo, if one exists
    async fn get_ref_by_symref(
        &self,
        _ctx: &CoreContext,
        symref: String,
    ) -> Result<Option<GitSymbolicRefsEntry>> {
        Ok(self
            .cached_entries
            .iter()
            .find(|entry| entry.symref_name == symref)
            .cloned())
    }

    /// Fetch the symrefs corresponding to the given ref name and type, if they exist
    async fn get_symrefs_by_ref(
        &self,
        _ctx: &CoreContext,
        ref_name: String,
        ref_type: RefType,
    ) -> Result<Option<Vec<String>>> {
        let symrefs = self
            .cached_entries
            .iter()
            .filter(|entry| entry.ref_name == ref_name && entry.ref_type == ref_type)
            .map(|entry| entry.symref_name.clone())
            .collect::<Vec<String>>();
        Ok((!symrefs.is_empty()).then_some(symrefs))
    }

    /// Add new symrefs to ref mappings or update existing symrefs
    async fn add_or_update_entries(
        &self,
        ctx: &CoreContext,
        entries: Vec<GitSymbolicRefsEntry>,
    ) -> Result<()> {
        self.git_symbolic_refs
            .add_or_update_entries(ctx, entries)
            .await
    }

    /// Delete symrefs if they exists
    async fn delete_symrefs(&self, ctx: &CoreContext, symrefs: Vec<String>) -> Result<()> {
        self.git_symbolic_refs.delete_symrefs(ctx, symrefs).await
    }

    /// List all symrefs for a given repo
    async fn list_all_symrefs(&self, _ctx: &CoreContext) -> Result<Vec<GitSymbolicRefsEntry>> {
        Ok(self.cached_entries.iter().cloned().collect())
    }
}
