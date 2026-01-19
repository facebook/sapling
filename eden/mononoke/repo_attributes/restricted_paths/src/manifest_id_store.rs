/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Display;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use derivative::Derivative;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use path_hash::PathBytes;
use path_hash::PathHash;
use path_hash::PathHashBytes;
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
use strum::Display as EnumDisplay;
use strum::EnumString;

type FromValueResult<T> = Result<T, FromValueError>;

// Create a newtype wrapper for SmallVec<[u8; 32]> to implement SQL traits
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ManifestId(SmallVec<[u8; 32]>);

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    EnumString,
    EnumDisplay,
    PartialOrd,
    Ord
)]
pub enum ManifestType {
    Hg,
    HgAugmented,
    Fsnode,
}

/// Entry representing a restricted path with its manifest type and id
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Derivative)]
#[derivative(Debug)]
pub struct RestrictedPathManifestIdEntry {
    pub manifest_type: ManifestType,
    pub manifest_id: ManifestId,
    #[derivative(Debug(format_with = "fmt_path_bytes"))]
    pub path: PathBytes,
    #[derivative(Debug(format_with = "fmt_path_hash_bytes"))]
    pub path_hash: PathHashBytes,
    // TODO(T239041722): add changeset id to log changeset to which the manifest belongs to
}

impl RestrictedPathManifestIdEntry {
    pub fn new(
        manifest_type: ManifestType,
        manifest_id: ManifestId,
        repo_path: RepoPath,
    ) -> Result<Self> {
        // Ensure that only directory paths are stored
        anyhow::ensure!(
            matches!(&repo_path, RepoPath::DirectoryPath(_)),
            "Path {repo_path} is not a non-root directory, so it can't be stored in the manifest store id"
        );

        let PathHash {
            path_bytes: path,
            hash: path_hash,
            ..
        } = PathHash::from_repo_path(&repo_path);
        Ok(Self {
            manifest_type,
            manifest_id,
            path,
            path_hash,
        })
    }

    /// Convert the stored PathBytes back to RepoPath (assumes DirectoryPath)
    pub fn repo_path(&self) -> Result<RepoPath> {
        RepoPath::dir(NonRootMPath::new(&self.path.0)?)
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
        entries: &[RestrictedPathManifestIdEntry],
    ) -> Result<bool>;

    /// Get all restricted paths that match a specific manifest id
    async fn get_paths_by_manifest_id(
        &self,
        ctx: &CoreContext,
        manifest_id: &ManifestId,
        manifest_type: &ManifestType,
        // TODO(T239041722): handle different paths with the same manifest id
    ) -> Result<Vec<NonRootMPath>>;

    /// Get all entries from the database
    async fn get_all_entries(
        &self,
        ctx: &CoreContext,
        // TODO(T239041722): add limit
    ) -> Result<Vec<RestrictedPathManifestIdEntry>>;

    fn repo_id(&self) -> RepositoryId;
}

mononoke_queries! {
    write InsertManifestIds(values: (
        repo_id: RepositoryId,
        manifest_type: ManifestType,
        manifest_id: ManifestId,
        path: PathBytes,
        path_hash: PathHashBytes,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO restricted_paths_manifest_ids
        (repo_id, manifest_type, manifest_id, path, path_hash)
        VALUES {values}
        "
    }

    read SelectPathsByManifestId(
        repo_id: RepositoryId,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
    ) -> (NonRootMPath) {
        "SELECT DISTINCT
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
        self.add_entries(ctx, &[entry]).await
    }

    async fn add_entries(
        &self,
        ctx: &CoreContext,
        entries: &[RestrictedPathManifestIdEntry],
    ) -> Result<bool> {
        let values: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.manifest_type,
                    &entry.manifest_id,
                    &entry.path,
                    &entry.path_hash,
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
        manifest_id: &ManifestId,
        manifest_type: &ManifestType,
    ) -> Result<Vec<NonRootMPath>> {
        let rows = SelectPathsByManifestId::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            manifest_id,
            manifest_type,
        )
        .await?;

        let result: Vec<NonRootMPath> = rows.into_iter().map(|row| row.0).collect();
        Ok(result)
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

        rows.into_iter()
            .map(|(manifest_type, manifest_id, non_root_mpath)| {
                let repo_path = RepoPath::DirectoryPath(non_root_mpath);
                RestrictedPathManifestIdEntry::new(manifest_type, manifest_id, repo_path)
            })
            .collect::<Result<_>>()
    }

    fn repo_id(&self) -> RepositoryId {
        self.repo_id
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
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
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
const HG_AUGMENTED: &[u8] = b"HgAugmented";
const FSNODE: &[u8] = b"Fsnode";

impl ConvIr<ManifestType> for ManifestType {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(ref b) if b == HG => Ok(ManifestType::Hg),
            Value::Bytes(ref b) if b == HG_AUGMENTED => Ok(ManifestType::HgAugmented),
            Value::Bytes(ref b) if b == FSNODE => Ok(ManifestType::Fsnode),
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

impl Display for ManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let st = hex::encode(&self.0);
        st.fmt(fmt)
    }
}

impl fmt::Debug for ManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "ManifestId({})", self)
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

impl From<&[u8; 32]> for ManifestId {
    fn from(bytes: &[u8; 32]) -> Self {
        let mut small_vec = SmallVec::new();
        small_vec.extend_from_slice(bytes);
        ManifestId(small_vec)
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

fn fmt_path_bytes(path: &PathBytes, f: &mut fmt::Formatter) -> fmt::Result {
    match std::str::from_utf8(&path.0) {
        Ok(path_str) => write!(f, "\"{}\"", path_str),
        Err(_) => write!(f, "PathBytes({:?})", path.0),
    }
}

fn fmt_path_hash_bytes(path_hash: &PathHashBytes, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "\"{}\"", hex::encode(&path_hash.0))
}
