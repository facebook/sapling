/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use smallvec::SmallVec;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::sql_common::mysql::OptionalTryFromRowField;
use sql::sql_common::mysql::RowField;
use sql::sql_common::mysql::ValueError;
use sql::sql_common::mysql::opt_try_from_rowfield;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;
use strum::Display;
use strum::EnumString;

type FromValueResult<T> = Result<T, FromValueError>;

// Create a newtype wrapper for SmallVec<[u8; 32]> to implement SQL traits
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ManifestId(SmallVec<[u8; 32]>);

#[derive(Clone, Debug, PartialEq, Eq, Hash, EnumString, Display)]
pub enum ManifestType {
    Hg,
}

/// Entry representing a restricted path with its manifest type and id  
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RestrictedPathManifestIdEntry {
    pub manifest_type: ManifestType,
    pub manifest_id: ManifestId,
    pub path: NonRootMPath,
}

impl RestrictedPathManifestIdEntry {
    pub fn new(manifest_type: ManifestType, manifest_id: ManifestId, path: NonRootMPath) -> Self {
        Self {
            manifest_type,
            manifest_id,
            path,
        }
    }
}

/// Interface for storing and fetching manifest ids from restricted paths.
#[facet::facet]
#[async_trait]
pub trait RestrictedPathsManifestIdStore: Send + Sync {
    /// Add a new restricted path manifest id entry to the database
    async fn add_entry(
        &self,
        ctx: &CoreContext,
        entry: RestrictedPathManifestIdEntry,
    ) -> Result<bool>;

    /// Add multiple restricted path manifest id entries to the database
    async fn add_entries(
        &self,
        ctx: &CoreContext,
        entries: Vec<RestrictedPathManifestIdEntry>,
    ) -> Result<bool>;

    /// Get all restricted paths that match a specific manifest id
    async fn get_paths_by_manifest_id(
        &self,
        ctx: &CoreContext,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
    ) -> Result<Vec<NonRootMPath>>;

    /// Get all entries from the database
    async fn get_all_entries(
        &self,
        ctx: &CoreContext,
        // TODO(T239041722): add limit
    ) -> Result<Vec<RestrictedPathManifestIdEntry>>;
}

mononoke_queries! {
    write InsertManifestIds(values: (
        repo_id: RepositoryId,
        manifest_type: ManifestType,
        manifest_id: ManifestId,
        path: NonRootMPath,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO restricted_paths_manifest_ids (repo_id, manifest_type, manifest_id, path) VALUES {values}"
    }

    read SelectPathsByManifestId(
        repo_id: RepositoryId,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
    ) -> (NonRootMPath) {
        "SELECT
            path
         FROM 
            restricted_paths_manifest_ids
         WHERE 
            repo_id = {repo_id}
            AND manifest_id = {manifest_id}
            AND manifest_type = {manifest_type}
        "
    }

    read SelectAllEntries(repo_id: RepositoryId) -> (ManifestType, ManifestId, NonRootMPath) {
        "SELECT
            manifest_type, 
            manifest_id, 
            path
         FROM 
            restricted_paths_manifest_ids
         WHERE
            repo_id = {repo_id}
         "
    }

}

pub struct SqlRestrictedPathsManifestIdStore {
    repo_id: RepositoryId,
    connections: SqlConnections,
}

impl SqlRestrictedPathsManifestIdStore {
    pub fn new(repo_id: RepositoryId, connections: SqlConnections) -> Self {
        Self {
            repo_id,
            connections,
        }
    }
}

#[async_trait]
impl RestrictedPathsManifestIdStore for SqlRestrictedPathsManifestIdStore {
    async fn add_entry(
        &self,
        ctx: &CoreContext,
        entry: RestrictedPathManifestIdEntry,
    ) -> Result<bool> {
        self.add_entries(ctx, vec![entry]).await
    }

    async fn add_entries(
        &self,
        ctx: &CoreContext,
        entries: Vec<RestrictedPathManifestIdEntry>,
    ) -> Result<bool> {
        let values: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.manifest_type,
                    &entry.manifest_id,
                    &entry.path,
                )
            })
            .collect();

        let result = InsertManifestIds::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &values[..],
        )
        .await?;

        Ok(result.affected_rows() > 0)
    }

    async fn get_paths_by_manifest_id(
        &self,
        ctx: &CoreContext,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
    ) -> Result<Vec<NonRootMPath>> {
        let rows = SelectPathsByManifestId::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &manifest_id,
            &manifest_type,
        )
        .await?;

        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    async fn get_all_entries(
        &self,
        ctx: &CoreContext,
    ) -> Result<Vec<RestrictedPathManifestIdEntry>> {
        let rows = SelectAllEntries::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
        )
        .await?;

        Ok(rows
            .into_iter()
            .map(|(manifest_type, manifest_id, path)| {
                RestrictedPathManifestIdEntry::new(manifest_type, manifest_id, path)
            })
            .collect())
    }
}

pub struct SqlRestrictedPathsManifestIdStoreBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlRestrictedPathsManifestIdStoreBuilder {
    const LABEL: &'static str = "restricted_paths_manifest_ids";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-restricted-paths-manifest-ids.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRestrictedPathsManifestIdStoreBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        remote.restricted_paths.as_ref()
    }
}

impl SqlRestrictedPathsManifestIdStoreBuilder {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlRestrictedPathsManifestIdStore {
        SqlRestrictedPathsManifestIdStore::new(repo_id, self.connections)
    }
}

// -----------------------------------------------------------------
// SQL Conversion

impl From<ManifestType> for Value {
    fn from(manifest_type: ManifestType) -> Self {
        Value::Bytes(manifest_type.to_string().as_bytes().to_vec())
    }
}

const HG: &[u8] = b"Hg";

impl ConvIr<ManifestType> for ManifestType {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(ref b) if b == HG => Ok(ManifestType::Hg),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for ManifestType {
    type Intermediate = ManifestType;
}

impl OptionalTryFromRowField for ManifestType {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}

impl ManifestId {
    pub fn new(data: SmallVec<[u8; 32]>) -> Self {
        Self(data)
    }

    pub fn into_inner(self) -> SmallVec<[u8; 32]> {
        self.0
    }

    pub fn as_inner(&self) -> &SmallVec<[u8; 32]> {
        &self.0
    }
}

impl From<SmallVec<[u8; 32]>> for ManifestId {
    fn from(data: SmallVec<[u8; 32]>) -> Self {
        Self(data)
    }
}

impl From<String> for ManifestId {
    fn from(hex_str: String) -> Self {
        match hex::decode(&hex_str) {
            Ok(bytes) => {
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(&bytes);
                Self(small_vec)
            }
            Err(_) => {
                // Fallback: treat as raw bytes if hex decoding fails
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(hex_str.as_bytes());
                Self(small_vec)
            }
        }
    }
}

impl From<&str> for ManifestId {
    fn from(hex_str: &str) -> Self {
        hex_str.to_string().into()
    }
}

impl From<ManifestId> for SmallVec<[u8; 32]> {
    fn from(id: ManifestId) -> Self {
        id.0
    }
}

// SQL conversion implementations for ManifestId
impl From<ManifestId> for Value {
    fn from(manifest_id: ManifestId) -> Self {
        Value::Bytes(manifest_id.0.to_vec())
    }
}

impl ConvIr<ManifestId> for ManifestId {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                // Fallback: treat as raw bytes
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(&bytes);
                Ok(ManifestId(small_vec))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for ManifestId {
    type Intermediate = ManifestId;
}

impl OptionalTryFromRowField for ManifestId {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}

// Implement conversion from tuple (which is what the SQL query returns)
impl From<(ManifestType, ManifestId, NonRootMPath)> for RestrictedPathManifestIdEntry {
    fn from((manifest_type, manifest_id, path): (ManifestType, ManifestId, NonRootMPath)) -> Self {
        Self::new(manifest_type, manifest_id, path)
    }
}
