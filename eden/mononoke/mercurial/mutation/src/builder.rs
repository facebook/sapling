/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use crate::store::SqlHgMutationStore;

const DEFAULT_MUTATION_CHAIN_LIMIT: usize = 500;

#[allow(unused)]
pub struct SqlHgMutationStoreBuilder {
    pub(crate) connections: SqlConnections,
    pub(crate) mutation_chain_limit: usize,
}

impl SqlConstruct for SqlHgMutationStoreBuilder {
    const LABEL: &'static str = "hg_mutations";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-hg-mutations.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections,
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
