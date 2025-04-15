/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql::Transaction;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;

use crate::Bundle;
use crate::BundleList;

mononoke_queries! {
    read GetLatestBundleListForRepo(repo_id: RepositoryId) -> (
        String, u64, u64, String
    ) {
    "SELECT bundle_handle, in_bundle_list_order, bundle_list, bundle_fingerprint
    FROM git_bundles
    WHERE repo_id = {repo_id}
    AND
    bundle_list = (
        SELECT MAX(bundle_list) FROM git_bundles where repo_id = {repo_id}
    )
    ORDER BY in_bundle_list_order ASC;
    "
    }

    read GetLatestBundleListNumForRepo(repo_id: RepositoryId) -> (
        u64
    ) {
    "SELECT MAX(bundle_list) FROM git_bundles where repo_id = {repo_id} group by repo_id having max(bundle_list) is not null"
    }

    read GetLatestBundleLists() -> (
        RepositoryId, String, u64, u64, String
    ) {
    "SELECT git_bundles.repo_id, bundle_handle, in_bundle_list_order, bundle_list, bundle_fingerprint
    FROM git_bundles
    JOIN (
        SELECT repo_id, max(bundle_list) as newest_bundle_list
        FROM git_bundles
        GROUP BY repo_id
    ) as SUBQUERY
    ON git_bundles.repo_id=SUBQUERY.repo_id and git_bundles.bundle_list=SUBQUERY.newest_bundle_list
    ORDER BY git_bundles.repo_id, in_bundle_list_order ASC;
    "
    }

    write AddNewBundles(values: (
        repo_id: RepositoryId,
        bundle_handle: String,
        bundle_list: u64,
        in_bundle_list_order: u64,
        bundle_fingerprint: String,
    ))  {
        none,
    "INSERT INTO git_bundles (repo_id, bundle_handle, bundle_list, in_bundle_list_order, bundle_fingerprint) VALUES {values}"
    }
}

#[derive(Clone)]
pub struct SqlGitBundleMetadataStorage {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlGitBundleMetadataStorageBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlGitBundleMetadataStorageBuilder {
    const LABEL: &'static str = "git_bundle_storage";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-git-bundle-metadata.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlGitBundleMetadataStorageBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        remote.git_bundle_metadata.as_ref()
    }
}

impl SqlGitBundleMetadataStorageBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlGitBundleMetadataStorage {
        SqlGitBundleMetadataStorage {
            connections: self.connections,
            repo_id,
        }
    }
}

impl SqlGitBundleMetadataStorage {
    /// Add new bundle-list to the DB as the latest bundle-list.
    /// The Bundles are expected to be in sorted order, increasingly, on
    /// Bundle.in_bundle_list_order.
    pub async fn add_new_bundles(&self, bundles: &[Bundle]) -> Result<()> {
        let conn = &self.connections.write_connection;
        let txn = conn.start_transaction().await?;

        let (txn, rows) =
            GetLatestBundleListNumForRepo::query_with_transaction(txn, &self.repo_id).await?;

        let new_bundle_list_num = rows.first().map_or(1, |val| val.0 + 1);
        let values: Vec<_> = bundles
            .iter()
            .map(|bundle| {
                (
                    &self.repo_id,
                    &bundle.handle,
                    &new_bundle_list_num,
                    &bundle.in_bundle_list_order,
                    &bundle.fingerprint,
                )
            })
            .collect();

        let (txn, _) = AddNewBundles::query_with_transaction(txn, values.as_slice()).await?;

        txn.commit().await?;

        Ok(())
    }

    pub async fn get_latest_bundle_list(&self) -> Result<Option<BundleList>> {
        let rows =
            GetLatestBundleListForRepo::query(&self.connections.read_connection, &self.repo_id)
                .await?;

        let bundle_list_num = match rows.first() {
            Some(val) => val.2,
            None => return Ok(None),
        };

        let bundles = rows
            .into_iter()
            .map(
                |(handle, in_bundle_list_order, _bundle_list, fingerprint)| Bundle {
                    in_bundle_list_order,
                    handle,
                    fingerprint,
                },
            )
            .collect();

        let bundle_list = BundleList {
            bundle_list_num,
            bundles,
        };

        Ok(Some(bundle_list))
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use fbinit::FacebookInit;
    use itertools::Itertools;
    use lazy_static::lazy_static;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use sql_construct::SqlConstruct;

    use super::Bundle;
    use super::SqlGitBundleMetadataStorageBuilder;

    lazy_static! {
        static ref TEST_BUNDLE_LIST_2: [Bundle; 2] = [
            Bundle {
                in_bundle_list_order: 1,
                handle: String::from("handle2137.1.1"),
                fingerprint: String::from("fingerprint2137.1.1"),
            },
            Bundle {
                in_bundle_list_order: 2,
                handle: String::from("handle2137.1.2"),
                fingerprint: String::from("fingerprint2137.1.2"),
            },
        ];
        static ref TEST_BUNDLE_LIST_3: [Bundle; 3] = [
            Bundle {
                in_bundle_list_order: 1,
                handle: String::from("handle2137.2.1"),
                fingerprint: String::from("fingerprint2137.2.1"),
            },
            Bundle {
                in_bundle_list_order: 2,
                handle: String::from("handle2137.2.2"),
                fingerprint: String::from("fingerprint2137.2.2"),
            },
            Bundle {
                in_bundle_list_order: 3,
                handle: String::from("handle2137.2.3"),
                fingerprint: String::from("fingerprint2137.2.3"),
            },
        ];
    }

    #[mononoke::fbinit_test]
    async fn test_no_bundles(_: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        let bundle_list = storage.get_latest_bundle_list().await?;
        assert!(bundle_list.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_add_bundles(_: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        storage.add_new_bundles(&TEST_BUNDLE_LIST_2[..]).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_latest_bundles(_: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        storage.add_new_bundles(&TEST_BUNDLE_LIST_2[..]).await?;

        let bundle_list = storage
            .get_latest_bundle_list()
            .await?
            .expect("Should return bundle-list");
        assert_eq!(bundle_list.bundles.len(), 2);
        assert_eq!(bundle_list.bundle_list_num, 1);
        for (p, n) in bundle_list.bundles.iter().tuple_windows() {
            assert!(p.in_bundle_list_order < n.in_bundle_list_order)
        }

        storage.add_new_bundles(&TEST_BUNDLE_LIST_3[..]).await?;

        let bundle_list = storage
            .get_latest_bundle_list()
            .await?
            .expect("Should return bundle-list");
        assert_eq!(bundle_list.bundles.len(), 3);
        assert_eq!(bundle_list.bundle_list_num, 2);
        for (p, n) in bundle_list.bundles.iter().tuple_windows() {
            assert!(p.in_bundle_list_order < n.in_bundle_list_order)
        }

        Ok(())
    }
}
