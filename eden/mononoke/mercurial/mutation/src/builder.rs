/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql::Connection as SqlConnection;
use sql::mysql::IsolationLevel;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;

use crate::store::SqlHgMutationStore;

const DEFAULT_MUTATION_CHAIN_LIMIT: usize = 20;

#[allow(unused)]
pub struct SqlHgMutationStoreBuilder {
    pub(crate) connections: SqlConnections,
    pub(crate) mutation_chain_limit: usize,
}

impl SqlConstruct for SqlHgMutationStoreBuilder {
    const LABEL: &'static str = "hg_mutations";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-hg-mutations.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let SqlConnections {
            read_connection,
            read_master_connection,
            mut write_connection,
        } = connections;

        if justknobs::eval("scm/mononoke:mutations_use_read_committed", None, None).unwrap_or(false)
        {
            if let Connection {
                inner: SqlConnection::Mysql(conn),
                ..
            } = &mut write_connection
            {
                conn.set_isolation_level(Some(IsolationLevel::ReadCommitted));
            }
        }
        Self {
            connections: SqlConnections {
                read_connection,
                read_master_connection,
                write_connection,
            },
            mutation_chain_limit: DEFAULT_MUTATION_CHAIN_LIMIT,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlHgMutationStoreBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.mutation)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
    }
}

impl SqlHgMutationStoreBuilder {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlHgMutationStore {
        SqlHgMutationStore::new(repo_id, self.connections, self.mutation_chain_limit)
    }

    pub fn with_mutation_limit(self, mutation_chain_limit: usize) -> Self {
        Self {
            mutation_chain_limit,
            ..self
        }
    }
}
