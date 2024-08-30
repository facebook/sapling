/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

use crate::GitSourceOfTruth;
use crate::GitSourceOfTruthConfig;
use crate::GitSourceOfTruthConfigEntry;
use crate::RepositoryName;
use crate::RowId;
use crate::Staleness;

mononoke_queries! {
    read TestGet(id: RowId) -> (
        RowId,
        RepositoryId,
        RepositoryName,
        GitSourceOfTruth,
    ) {
        "SELECT id,
            repo_id,
            repo_name,
            source_of_truth
         FROM git_repositories_source_of_truth
         WHERE id = {id}"
    }

    read GetByRepoId(repo_id: RepositoryId) -> (
        RowId,
        RepositoryId,
        RepositoryName,
        GitSourceOfTruth,
    ) {
        "SELECT id,
            repo_id,
            repo_name,
            source_of_truth
         FROM git_repositories_source_of_truth
         WHERE repo_id = {repo_id}"
    }

    read GetByGitSourceOfTruth(source_of_truth: GitSourceOfTruth) -> (
        RowId,
        RepositoryId,
        RepositoryName,
        GitSourceOfTruth,
    ) {
        "SELECT id,
            repo_id,
            repo_name,
            source_of_truth
         FROM git_repositories_source_of_truth
         WHERE source_of_truth = {source_of_truth}"
    }

    write Set(repo_id: RepositoryId, repo_name: RepositoryName, source_of_truth: GitSourceOfTruth) {
        none,
        mysql("INSERT INTO git_repositories_source_of_truth (repo_id, repo_name, source_of_truth) VALUES ({repo_id}, {repo_name}, {source_of_truth}) ON DUPLICATE KEY UPDATE source_of_truth = {source_of_truth}")
        sqlite("REPLACE INTO git_repositories_source_of_truth (repo_id, repo_name, source_of_truth) VALUES ({repo_id}, {repo_name}, {source_of_truth})")
    }
}

fn row_to_entry(
    row: (RowId, RepositoryId, RepositoryName, GitSourceOfTruth),
) -> GitSourceOfTruthConfigEntry {
    let (id, repo_id, repo_name, source_of_truth) = row;
    GitSourceOfTruthConfigEntry {
        id,
        repo_id,
        repo_name,
        source_of_truth,
    }
}

pub struct SqlGitSourceOfTruthConfig {
    connections: SqlConnections,
}

impl SqlGitSourceOfTruthConfig {
    pub fn get_connection(&self, staleness: Staleness) -> &Connection {
        match staleness {
            Staleness::MostRecent => &self.connections.read_master_connection,
            Staleness::MaybeStale => &self.connections.read_connection,
        }
    }
}

#[derive(Clone)]
pub struct SqlGitSourceOfTruthConfigBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlGitSourceOfTruthConfigBuilder {
    const LABEL: &'static str = "git_repositories_source_of_truth";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-git-repositories-source-of-truth.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlGitSourceOfTruthConfigBuilder {
    pub fn build(self) -> SqlGitSourceOfTruthConfig {
        let SqlGitSourceOfTruthConfigBuilder { connections } = self;

        SqlGitSourceOfTruthConfig { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlGitSourceOfTruthConfigBuilder {}

#[async_trait]
impl GitSourceOfTruthConfig for SqlGitSourceOfTruthConfig {
    async fn set(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        repo_name: RepositoryName,
        source_of_truth: GitSourceOfTruth,
    ) -> Result<()> {
        Set::query(
            &self.connections.write_connection,
            &repo_id,
            &repo_name,
            &source_of_truth,
        )
        .await?;
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<GitSourceOfTruthConfigEntry>> {
        let rows = GetByRepoId::query(self.get_connection(staleness), &repo_id).await?;
        Ok(rows.into_iter().next().map(row_to_entry))
    }

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        let rows = GetByGitSourceOfTruth::query(
            &self.connections.read_master_connection,
            &GitSourceOfTruth::Mononoke,
        )
        .await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        let rows = GetByGitSourceOfTruth::query(
            &self.connections.read_master_connection,
            &GitSourceOfTruth::Metagit,
        )
        .await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }

    async fn get_locked(&self, _ctx: &CoreContext) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        let rows = GetByGitSourceOfTruth::query(
            &self.connections.read_master_connection,
            &GitSourceOfTruth::Locked,
        )
        .await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }
}

#[cfg(all(fbcode_build, test))]
mod test {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_set(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.clone().build();

        // insert one
        let repo_id = RepositoryId::new(1);
        let repo_name = RepositoryName("test1".to_string());
        push.set(&ctx, repo_id, repo_name.clone(), GitSourceOfTruth::Mononoke)
            .await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.source_of_truth, GitSourceOfTruth::Mononoke);

        let push = builder.build();

        // insert another
        let repo_id = RepositoryId::new(2);
        let repo_name = RepositoryName("test2".to_string());
        push.set(&ctx, repo_id, repo_name.clone(), GitSourceOfTruth::Metagit)
            .await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.source_of_truth, GitSourceOfTruth::Metagit);

        // update it
        push.set(&ctx, repo_id, repo_name, GitSourceOfTruth::Mononoke)
            .await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.source_of_truth, GitSourceOfTruth::Mononoke);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_by_repo_id(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.build();

        let repo_id = RepositoryId::new(1);
        let repo_name = RepositoryName("test1".to_string());
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_none());

        push.set(&ctx, repo_id, repo_name, GitSourceOfTruth::Mononoke)
            .await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.source_of_truth, GitSourceOfTruth::Mononoke);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_redirected(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.build();

        let to_be_redirected_to_mononoke_repo_id = RepositoryId::new(1);
        let to_be_redirected_to_mononoke_repo_name = RepositoryName("test1".to_string());
        push.set(
            &ctx,
            to_be_redirected_to_mononoke_repo_id,
            to_be_redirected_to_mononoke_repo_name.clone(),
            GitSourceOfTruth::Metagit,
        )
        .await?;
        push.set(
            &ctx,
            RepositoryId::new(2),
            RepositoryName("test2".to_string()),
            GitSourceOfTruth::Mononoke,
        )
        .await?;
        push.set(
            &ctx,
            RepositoryId::new(3),
            RepositoryName("test3".to_string()),
            GitSourceOfTruth::Metagit,
        )
        .await?;

        assert_eq!(push.get_redirected_to_mononoke(&ctx).await?.len(), 1);
        assert_eq!(push.get_redirected_to_metagit(&ctx).await?.len(), 2);

        push.set(
            &ctx,
            to_be_redirected_to_mononoke_repo_id,
            to_be_redirected_to_mononoke_repo_name,
            GitSourceOfTruth::Mononoke,
        )
        .await?;

        assert_eq!(push.get_redirected_to_mononoke(&ctx).await?.len(), 2);
        assert_eq!(push.get_redirected_to_metagit(&ctx).await?.len(), 1);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_locked(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.build();

        let to_be_locked_repo_id = RepositoryId::new(1);
        let to_be_locked_repo_name = RepositoryName("test1".to_string());
        push.set(
            &ctx,
            to_be_locked_repo_id,
            to_be_locked_repo_name.clone(),
            GitSourceOfTruth::Locked,
        )
        .await?;
        push.set(
            &ctx,
            RepositoryId::new(2),
            RepositoryName("test2".to_string()),
            GitSourceOfTruth::Locked,
        )
        .await?;

        assert_eq!(push.get_locked(&ctx).await?.len(), 2);
        Ok(())
    }
}
