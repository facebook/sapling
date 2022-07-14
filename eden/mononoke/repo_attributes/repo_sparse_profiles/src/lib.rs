/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use sql::queries;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::collections::HashMap;

queries! {
    read GetProfilesSize(
        cs_id: ChangesetId,
        >list profiles: String
    ) -> (String, u64) {
        "SELECT profile_name, size
          FROM sparse_profiles_sizes
          WHERE cs_id = {cs_id}
          AND profile_name in {profiles}"
    }

    write InsertProfilesSizes(
        values: (cs_id: ChangesetId, profile: String, size: u64),
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO sparse_profiles_sizes
         (cs_id, profile_name, size) VALUES {values}"
    }
}

#[facet::facet]
pub struct RepoSparseProfiles {
    pub sql_profile_sizes: Option<SqlSparseProfilesSizes>,
}

impl RepoSparseProfiles {
    pub fn new(sql_profile_sizes: Option<SqlSparseProfilesSizes>) -> Self {
        Self { sql_profile_sizes }
    }

    pub async fn get_profiles_sizes(
        &self,
        cs_id: ChangesetId,
        profiles: Vec<String>,
    ) -> Result<Option<Vec<(String, u64)>>> {
        Ok(match &self.sql_profile_sizes {
            None => None,
            Some(sql) => Some(sql.get_profiles_sizes(cs_id, profiles).await?),
        })
    }

    pub async fn insert_profiles_sizes(
        &self,
        cs_id: ChangesetId,
        size_map: HashMap<String, u64>,
    ) -> Result<Option<bool>> {
        Ok(match &self.sql_profile_sizes {
            None => None,
            Some(sql) => Some(sql.insert_profiles_sizes(cs_id, size_map).await?),
        })
    }
}

pub struct SqlSparseProfilesSizes {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstruct for SqlSparseProfilesSizes {
    const LABEL: &'static str = "sparse_profiles_sizes";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-sparse-profiles-sizes.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlSparseProfilesSizes {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.sparse_profiles)
    }
}

impl SqlSparseProfilesSizes {
    pub async fn get_profiles_sizes(
        &self,
        cs_id: ChangesetId,
        profiles: Vec<String>,
    ) -> Result<Vec<(String, u64)>> {
        GetProfilesSize::query(&self.read_connection, &cs_id, &profiles[..]).await
    }

    pub async fn insert_profiles_sizes(
        &self,
        cs_id: ChangesetId,
        size_map: HashMap<String, u64>,
    ) -> Result<bool> {
        let v: Vec<_> = size_map
            .iter()
            .map(|(profile, size)| (&cs_id, profile, size))
            .collect();
        InsertProfilesSizes::query(&self.write_connection, &v[..])
            .await
            .map(|res| res.affected_rows() > 0)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    #[tokio::test]
    async fn test_simple() -> Result<()> {
        let sql = SqlSparseProfilesSizes::with_sqlite_in_memory()?;

        let mut size = 10;
        for cs in &[ONES_CSID, THREES_CSID, TWOS_CSID] {
            let m: HashMap<_, _> = ["one", "two", "three"]
                .iter()
                .map(|profile| {
                    size += 5;
                    (profile.to_string(), size)
                })
                .collect();
            sql.insert_profiles_sizes(*cs, m).await?;
        }
        let rows = sql
            .get_profiles_sizes(TWOS_CSID, vec!["one".to_string(), "three".to_string()])
            .await?;
        assert_eq!(
            rows,
            vec![("one".to_string(), 45), ("three".to_string(), 55)]
        );
        Ok(())
    }
}
