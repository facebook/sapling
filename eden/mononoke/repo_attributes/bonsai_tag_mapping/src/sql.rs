/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use ::sql_ext::Connection;
use ::sql_ext::mononoke_queries;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::BonsaiTagMapping;
use super::BonsaiTagMappingEntry;
use crate::Freshness;

#[derive(Clone)]
struct RendezVousConnection {
    changeset: RendezVous<ChangesetId, Vec<(String, GitSha1, bool)>>,
    tag_hash: RendezVous<GitSha1, Vec<(String, ChangesetId, bool)>>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            changeset: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_tag_mapping.changeset.{}",
                    name,
                ))),
            ),
            tag_hash: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_tag_mapping.tag_hash.{}",
                    name,
                ))),
            ),
        }
    }
}

mononoke_queries! {
    write AddOrUpdateBonsaiTagMapping(values: (
        repo_id: RepositoryId,
        tag_name: String,
        changeset_id: ChangesetId,
        tag_hash: GitSha1,
        target_is_tag: bool,
    )) {
        none,
        "REPLACE INTO bonsai_tag_mapping (repo_id, tag_name, changeset_id, tag_hash, target_is_tag) VALUES {values}"
    }

    write DeleteBonsaiTagMappingsByName(repo_id: RepositoryId,
        >list tag_names: String) {
        none,
        "DELETE FROM bonsai_tag_mapping WHERE repo_id = {repo_id} AND tag_name IN {tag_names}"
    }

    read SelectAllMappings(
        repo_id: RepositoryId,
    ) -> (String, ChangesetId, GitSha1, bool) {
        "SELECT tag_name, changeset_id, tag_hash, target_is_tag
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id}"
    }

    read SelectMappingByChangeset(
        repo_id: RepositoryId,
        >list changeset_id: ChangesetId
    ) -> (String, ChangesetId, GitSha1, bool) {
        "SELECT tag_name, changeset_id, tag_hash, target_is_tag
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND changeset_id IN {changeset_id}"
    }

    read SelectMappingByTagName(
        repo_id: RepositoryId,
        tag_name: String,
    ) -> (String, ChangesetId, GitSha1, bool) {
        "SELECT tag_name, changeset_id, tag_hash, target_is_tag
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND tag_name = {tag_name}"
    }

    read SelectMappingByTagHash(
        repo_id: RepositoryId,
        >list tag_hash: GitSha1
    ) -> (String, ChangesetId, GitSha1, bool) {
        "SELECT tag_name, changeset_id, tag_hash, target_is_tag
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND tag_hash IN {tag_hash}"
    }
}

pub struct SqlBonsaiTagMapping {
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiTagMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiTagMappingBuilder {
    const LABEL: &'static str = "bonsai_tag_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-tag-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiTagMappingBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
}

impl SqlBonsaiTagMappingBuilder {
    pub fn build(self, repo_id: RepositoryId, opts: RendezVousOptions) -> SqlBonsaiTagMapping {
        let SqlBonsaiTagMappingBuilder { connections } = self;
        SqlBonsaiTagMapping {
            write_connection: connections.write_connection,
            read_connection: RendezVousConnection::new(connections.read_connection, "reader", opts),
            read_master_connection: RendezVousConnection::new(
                connections.read_master_connection,
                "read_master",
                opts,
            ),
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiTagMapping for SqlBonsaiTagMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<BonsaiTagMappingEntry>> {
        let results = SelectAllMappings::query(
            &self.read_connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
        )
        .await
        .with_context(|| format!("Failure in fetching all entries for repo {}", self.repo_id))?;

        let values = results
            .into_iter()
            .map(|(tag_name, changeset_id, tag_hash, target_is_tag)| {
                BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
            })
            .collect::<Vec<_>>();
        return Ok(values);
    }

    async fn get_entry_by_tag_name(
        &self,
        ctx: &CoreContext,
        tag_name: String,
        freshness: Freshness,
    ) -> Result<Option<BonsaiTagMappingEntry>> {
        let connection = if freshness == Freshness::Latest {
            &self.read_master_connection
        } else {
            &self.read_connection
        };
        let results = SelectMappingByTagName::query(
            &connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &tag_name,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching entry for tag {} in repo {}",
                tag_name, self.repo_id
            )
        })?;
        // This should not happen but since this is new code, extra checks dont hurt.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple entries returned for tag {} in repo {}",
                tag_name,
                self.repo_id
            )
        }
        Ok(results
            .into_iter()
            .next()
            .map(|(tag_name, changeset_id, tag_hash, target_is_tag)| {
                BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
            }))
    }

    async fn get_entries_by_changesets(
        &self,
        ctx: &CoreContext,
        changeset_ids: Vec<ChangesetId>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        select_mapping_by_changeset(ctx, &self.read_connection, &self.repo_id, changeset_ids).await
    }

    async fn get_entries_by_tag_hashes(
        &self,
        ctx: &CoreContext,
        tag_hashes: Vec<GitSha1>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        select_mapping_by_tag_hash(ctx, &self.read_connection, &self.repo_id, tag_hashes).await
    }

    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<BonsaiTagMappingEntry>,
    ) -> Result<()> {
        let converted_entries: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.tag_name,
                    &entry.changeset_id,
                    &entry.tag_hash,
                    &entry.target_is_tag,
                )
            })
            .collect();
        AddOrUpdateBonsaiTagMapping::query(
            &self.write_connection,
            ctx.sql_query_telemetry(),
            converted_entries.as_slice(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to add mappings in repo {} for entries {:?}",
                self.repo_id, entries,
            )
        })?;
        Ok(())
    }

    async fn delete_mappings_by_name(
        &self,
        ctx: &CoreContext,
        tag_names: Vec<String>,
    ) -> Result<()> {
        DeleteBonsaiTagMappingsByName::query(
            &self.write_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            tag_names.as_slice(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to delete mappings in repo {} for tag names {:?}",
                self.repo_id, tag_names,
            )
        })?;
        Ok(())
    }
}

async fn select_mapping_by_changeset(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    changeset_ids: Vec<ChangesetId>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    if changeset_ids.is_empty() {
        return Ok(vec![]);
    }

    let use_rendezvous = justknobs::eval(
        "scm/mononoke:rendezvous_bonsai_tag_mapping",
        ctx.client_correlator(),
        None,
    )?;

    if use_rendezvous {
        select_mapping_by_changeset_rendezvous(ctx, connection, repo_id, changeset_ids).await
    } else {
        select_mapping_by_changeset_non_rendezvous(ctx, &connection.conn, repo_id, changeset_ids)
            .await
    }
}

async fn select_mapping_by_changeset_rendezvous(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    changeset_ids: Vec<ChangesetId>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    let ret = connection
        .changeset
        .dispatch(ctx.fb, changeset_ids.iter().copied().collect(), || {
            let repo_id = *repo_id;
            let conn = connection.conn.clone();
            let telemetry = ctx.sql_query_telemetry().clone();
            move |changeset_ids: HashSet<ChangesetId>| async move {
                let changeset_ids = changeset_ids.into_iter().collect::<Vec<_>>();
                let res =
                    SelectMappingByChangeset::query(&conn, telemetry, &repo_id, &changeset_ids[..])
                        .await?;

                let mut result: std::collections::HashMap<
                    ChangesetId,
                    Vec<(String, GitSha1, bool)>,
                > = std::collections::HashMap::new();
                for (tag_name, changeset_id, tag_hash, target_is_tag) in res {
                    result.entry(changeset_id).or_default().push((
                        tag_name,
                        tag_hash,
                        target_is_tag,
                    ));
                }
                Ok(result)
            }
        })
        .await?;

    let entries: Vec<BonsaiTagMappingEntry> = ret
        .into_iter()
        .flat_map(|(changeset_id, tags)| {
            tags.map(|tags| {
                tags.into_iter()
                    .map(move |(tag_name, tag_hash, target_is_tag)| {
                        BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
                    })
            })
        })
        .flatten()
        .collect();

    Ok(entries)
}

async fn select_mapping_by_changeset_non_rendezvous(
    ctx: &CoreContext,
    connection: &Connection,
    repo_id: &RepositoryId,
    changeset_ids: Vec<ChangesetId>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    let results = SelectMappingByChangeset::query(
        connection,
        ctx.sql_query_telemetry(),
        repo_id,
        changeset_ids.as_slice(),
    )
    .await
    .with_context(|| {
        format!(
            "Failure in fetching entry for changesets {:?} in repo {}",
            changeset_ids, repo_id
        )
    })?;

    let values = results
        .into_iter()
        .map(|(tag_name, changeset_id, tag_hash, target_is_tag)| {
            BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
        })
        .collect::<Vec<_>>();
    Ok(values)
}

async fn select_mapping_by_tag_hash(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    tag_hashes: Vec<GitSha1>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    if tag_hashes.is_empty() {
        return Ok(vec![]);
    }

    let use_rendezvous = justknobs::eval(
        "scm/mononoke:rendezvous_bonsai_tag_mapping",
        ctx.client_correlator(),
        None,
    )?;

    if use_rendezvous {
        select_mapping_by_tag_hash_rendezvous(ctx, connection, repo_id, tag_hashes).await
    } else {
        select_mapping_by_tag_hash_non_rendezvous(ctx, &connection.conn, repo_id, tag_hashes).await
    }
}

async fn select_mapping_by_tag_hash_rendezvous(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    tag_hashes: Vec<GitSha1>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    let ret = connection
        .tag_hash
        .dispatch(ctx.fb, tag_hashes.iter().copied().collect(), || {
            let repo_id = *repo_id;
            let conn = connection.conn.clone();
            let telemetry = ctx.sql_query_telemetry().clone();
            move |tag_hashes: HashSet<GitSha1>| async move {
                let tag_hashes = tag_hashes.into_iter().collect::<Vec<_>>();
                let res =
                    SelectMappingByTagHash::query(&conn, telemetry, &repo_id, &tag_hashes[..])
                        .await?;

                let mut result: std::collections::HashMap<
                    GitSha1,
                    Vec<(String, ChangesetId, bool)>,
                > = std::collections::HashMap::new();
                for (tag_name, changeset_id, tag_hash, target_is_tag) in res {
                    result.entry(tag_hash).or_default().push((
                        tag_name,
                        changeset_id,
                        target_is_tag,
                    ));
                }
                Ok(result)
            }
        })
        .await?;

    let entries: Vec<BonsaiTagMappingEntry> = ret
        .into_iter()
        .flat_map(|(tag_hash, tags)| {
            tags.map(|tags| {
                tags.into_iter()
                    .map(move |(tag_name, changeset_id, target_is_tag)| {
                        BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
                    })
            })
        })
        .flatten()
        .collect();

    Ok(entries)
}

async fn select_mapping_by_tag_hash_non_rendezvous(
    ctx: &CoreContext,
    connection: &Connection,
    repo_id: &RepositoryId,
    tag_hashes: Vec<GitSha1>,
) -> Result<Vec<BonsaiTagMappingEntry>> {
    let results = SelectMappingByTagHash::query(
        connection,
        ctx.sql_query_telemetry(),
        repo_id,
        tag_hashes.as_slice(),
    )
    .await
    .with_context(|| {
        format!(
            "Failure in fetching entry for tag hashes {:?} in repo {}",
            tag_hashes, repo_id
        )
    })?;

    let values = results
        .into_iter()
        .map(|(tag_name, changeset_id, tag_hash, target_is_tag)| {
            BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash, target_is_tag)
        })
        .collect::<Vec<_>>();
    Ok(values)
}
