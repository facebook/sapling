/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod sql;

use std::fmt::Display;

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::RepositoryId;

pub use crate::sql::SqlGitSymbolicRefs;
pub use crate::sql::SqlGitSymbolicRefsBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RefType {
    Branch,
    Tag,
}

impl TryFrom<&str> for RefType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "branch" => Ok(RefType::Branch),
            "tag" => Ok(RefType::Tag),
            ty => Err(anyhow::anyhow!(
                "Unknown ref type {} as target of symref",
                ty
            )),
        }
    }
}

impl Display for RefType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefType::Branch => f.write_str("branch"),
            RefType::Tag => f.write_str("tag"),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GitSymbolicRefsEntry {
    pub symref_name: String,
    pub ref_name: String,
    pub ref_type: RefType,
}

impl GitSymbolicRefsEntry {
    pub fn new(symref_name: String, ref_name: String, ref_type: String) -> Result<Self> {
        let ref_type = ref_type.as_str().try_into()?;
        Ok(GitSymbolicRefsEntry {
            symref_name,
            ref_name,
            ref_type,
        })
    }

    pub fn ref_name_with_type(&self) -> String {
        match self.ref_type {
            RefType::Branch => format!("refs/heads/{}", self.ref_name),
            RefType::Tag => format!("refs/tags/{}", self.ref_name),
        }
    }
}

#[facet::facet]
#[async_trait]
/// Facet trait representing Git Symbolic Refs for the repo
pub trait GitSymbolicRefs: Send + Sync {
    /// The repository for which these symrefs exist
    fn repo_id(&self) -> RepositoryId;

    /// Fetch the symbolic ref entry corresponding to the symref name in the
    /// given repo, if one exists
    async fn get_ref_by_symref(&self, symref: String) -> Result<Option<GitSymbolicRefsEntry>>;

    /// Fetch the symrefs corresponding to the given ref name and type, if they exist
    async fn get_symrefs_by_ref(
        &self,
        ref_name: String,
        ref_type: RefType,
    ) -> Result<Option<Vec<String>>>;

    /// Add new symrefs to ref mappings or update existing symrefs
    async fn add_or_update_entries(&self, entries: Vec<GitSymbolicRefsEntry>) -> Result<()>;

    /// Delete symrefs if they exists
    async fn delete_symrefs(&self, symrefs: Vec<String>) -> Result<()>;

    /// List all symrefs for a given repo
    async fn list_all_symrefs(&self) -> Result<Vec<GitSymbolicRefsEntry>>;
}
