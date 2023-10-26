/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::GitSymbolicRefs;
use super::GitSymbolicRefsEntry;
use super::RefType;

mononoke_queries! {
    write AddOrUpdateGitSymbolicRefs(values: (
        repo_id: RepositoryId,
        symref_name: String,
        ref_name: String,
        ref_type: String,
    )) {
        none,
        "REPLACE INTO git_symbolic_refs (repo_id, symref_name, ref_name, ref_type) VALUES {values}"
    }

    write DeleteGitSymbolicRefs(
        repo_id: RepositoryId,
        >list symrefs: String
    ) {
        none,
        "DELETE FROM git_symbolic_refs WHERE
        repo_id = {repo_id} AND symref_name IN {symrefs}"
    }

    read SelectAllGitSymbolicRefs(
        repo_id: RepositoryId
    ) -> (String, String, String) {
        "SELECT symref_name, ref_name, ref_type
         FROM git_symbolic_refs
         WHERE repo_id = {repo_id}"
    }

    read SelectRefBySymref(
        repo_id: RepositoryId,
        symref_name: String
    ) -> (String, String, String) {
        "SELECT symref_name, ref_name, ref_type
          FROM git_symbolic_refs
          WHERE repo_id = {repo_id} AND symref_name = {symref_name}"
    }

    read SelectSymrefsByRef(
        repo_id: RepositoryId,
        ref_name: String,
        ref_type: String
    ) -> (String) {
        "SELECT symref_name
          FROM git_symbolic_refs
          WHERE repo_id = {repo_id} AND ref_name = {ref_name} AND ref_type = {ref_type}"
    }
}

pub struct SqlGitSymbolicRefs {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlGitSymbolicRefsBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlGitSymbolicRefsBuilder {
    const LABEL: &'static str = "git_symbolic_refs";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-git-symbolic-refs.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlGitSymbolicRefsBuilder {}

impl SqlGitSymbolicRefsBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlGitSymbolicRefs {
        SqlGitSymbolicRefs {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl GitSymbolicRefs for SqlGitSymbolicRefs {
    /// The repository for which these symrefs exist
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    /// Fetch the symbolic ref entry corresponding to the symref name in the
    /// given repo, if one exists
    async fn get_ref_by_symref(&self, symref: String) -> Result<Option<GitSymbolicRefsEntry>> {
        let results =
            SelectRefBySymref::query(&self.connections.read_connection, &self.repo_id, &symref)
                .await
                .with_context(|| {
                    format!(
                        "Failure in fetching ref for symref {} in repo {}",
                        symref, self.repo_id
                    )
                })?;
        // This should not happen but since this is new code, extra checks dont hurt.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple refs returned for symref {} in repo {}",
                symref,
                self.repo_id
            )
        }
        results
            .into_iter()
            .next()
            .map(|(symref_name, ref_name, ref_type)| {
                GitSymbolicRefsEntry::new(symref_name, ref_name, ref_type)
            })
            .transpose()
    }

    /// Fetch the symrefs corresponding to the given ref name and type, if they exist
    async fn get_symrefs_by_ref(
        &self,
        ref_name: String,
        ref_type: RefType,
    ) -> Result<Option<Vec<String>>> {
        let results = SelectSymrefsByRef::query(
            &self.connections.read_connection,
            &self.repo_id,
            &ref_name,
            &ref_type.to_string(),
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching symrefs for ref name {} and type {} in repo {}",
                ref_name, ref_type, self.repo_id
            )
        })?;

        let values = results
            .into_iter()
            .map(|(symref,)| symref)
            .collect::<Vec<_>>();
        let output = (!values.is_empty()).then_some(values);
        Ok(output)
    }

    /// Add new symrefs to ref mappings or update existing symrefs
    async fn add_or_update_entries(&self, entries: Vec<GitSymbolicRefsEntry>) -> Result<()> {
        let entries: Vec<_> = entries
            .into_iter()
            .map(|entry| {
                (
                    self.repo_id,
                    entry.symref_name,
                    entry.ref_name,
                    entry.ref_type.to_string(),
                )
            })
            .collect();
        let entries: Vec<_> = entries
            .iter()
            .map(|(repo_id, symref_name, ref_name, ref_type)| {
                (repo_id, symref_name, ref_name, ref_type)
            })
            .collect();
        AddOrUpdateGitSymbolicRefs::query(&self.connections.write_connection, entries.as_slice())
            .await
            .with_context(|| {
                format!(
                    "Failed to add mappings in repo {} for entries {:?}",
                    self.repo_id, entries,
                )
            })?;
        Ok(())
    }

    /// Delete the entry corresponding to the given symref if its exists
    async fn delete_symrefs(&self, symref: Vec<String>) -> Result<()> {
        DeleteGitSymbolicRefs::query(
            &self.connections.write_connection,
            &self.repo_id,
            symref.as_slice(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to delete symrefs {:?} in repo {}",
                symref, self.repo_id
            )
        })?;
        Ok(())
    }

    /// List all symrefs for a given repo
    async fn list_all_symrefs(&self) -> Result<Vec<GitSymbolicRefsEntry>> {
        let results =
            SelectAllGitSymbolicRefs::query(&self.connections.read_connection, &self.repo_id)
                .await
                .with_context(|| {
                    format!(
                        "Failure in fetching git symbolic refs in repo {}",
                        self.repo_id
                    )
                })?;
        results
            .into_iter()
            .map(|(symref_name, ref_name, ref_type)| {
                GitSymbolicRefsEntry::new(symref_name, ref_name, ref_type)
            })
            .collect()
    }
}
