/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use sql_ext::SqlQueryTelemetry;
use sql_ext::Transaction;
use sql_ext::mononoke_queries;

use crate::Bundle;
use crate::BundleList;

mononoke_queries! {
    read GetLatestBundleListForRepo(repo_id: RepositoryId) -> (
        String, u64, u64, String, u64
    ) {
    "SELECT bundle_handle, in_bundle_list_order, bundle_list, bundle_fingerprint, generation_start_timestamp
    FROM git_bundles
    WHERE repo_id = {repo_id}
    AND
    bundle_list = (
        SELECT MAX(bundle_list) FROM git_bundles where repo_id = {repo_id}
    )
    ORDER BY in_bundle_list_order ASC;
    "
    }

    read GetBundleListsForRepo(repo_id: RepositoryId) -> (
        String, u64, u64, String, u64
    ) {
    "SELECT bundle_handle, in_bundle_list_order, bundle_list, bundle_fingerprint, generation_start_timestamp
    FROM git_bundles
    WHERE repo_id = {repo_id}
    ORDER BY bundle_list DESC, in_bundle_list_order ASC;
    "
    }

    read GetLatestBundleListNumForRepo(repo_id: RepositoryId) -> (
        u64
    ) {
    "SELECT MAX(bundle_list) FROM git_bundles where repo_id = {repo_id} group by repo_id having max(bundle_list) is not null"
    }

    read GetLatestBundleLists() -> (
        RepositoryId, String, u64, u64, String, u64
    ) {
    "SELECT git_bundles.repo_id, bundle_handle, in_bundle_list_order, bundle_list, bundle_fingerprint, generation_start_timestamp
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
        generation_start_timestamp: u64,
    ))  {
        none,
    "INSERT INTO git_bundles (repo_id, bundle_handle, bundle_list, in_bundle_list_order, bundle_fingerprint, generation_start_timestamp) VALUES {values}"
    }

    write RemoveBundleList(repo_id: RepositoryId, bundle_list: u64) {
        none,
    "DELETE FROM git_bundles WHERE repo_id = {repo_id} and bundle_list = {bundle_list}"
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
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
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
    pub async fn add_new_bundles(&self, ctx: &CoreContext, bundles: &[Bundle]) -> Result<u64> {
        let conn = &self.connections.write_connection;
        let txn = conn.start_transaction(ctx.sql_query_telemetry()).await?;

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
                    &bundle.generation_start_timestamp,
                )
            })
            .collect();

        let (txn, _) = AddNewBundles::query_with_transaction(txn, values.as_slice()).await?;

        txn.commit().await?;

        Ok(new_bundle_list_num)
    }

    pub async fn get_latest_bundle_list_from_primary(
        &self,
        ctx: &CoreContext,
    ) -> Result<Option<BundleList>> {
        self._get_latest_bundle_list(ctx, true).await
    }

    pub async fn get_latest_bundle_list(&self, ctx: &CoreContext) -> Result<Option<BundleList>> {
        self._get_latest_bundle_list(ctx, false).await
    }
    pub async fn _get_latest_bundle_list(
        &self,
        ctx: &CoreContext,
        read_from_primary: bool,
    ) -> Result<Option<BundleList>> {
        let conn = if read_from_primary {
            &self.connections.write_connection
        } else {
            &self.connections.read_connection
        };
        let rows =
            GetLatestBundleListForRepo::query(conn, ctx.sql_query_telemetry(), &self.repo_id)
                .await?;

        let bundle_list_num = match rows.first() {
            Some(val) => val.2,
            None => return Ok(None),
        };

        let bundles = rows
            .into_iter()
            .map(
                |(
                    handle,
                    in_bundle_list_order,
                    _bundle_list,
                    fingerprint,
                    generation_start_timestamp,
                )| Bundle {
                    in_bundle_list_order,
                    handle,
                    fingerprint,
                    generation_start_timestamp,
                },
            )
            .collect();

        let bundle_list = BundleList {
            bundle_list_num,
            bundles,
        };

        Ok(Some(bundle_list))
    }

    pub async fn remove_bundle_list(&self, ctx: &CoreContext, bundle_list_num: u64) -> Result<()> {
        RemoveBundleList::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &bundle_list_num,
        )
        .await?;
        Ok(())
    }

    pub async fn get_bundle_lists(&self, ctx: &CoreContext) -> Result<Vec<BundleList>> {
        let rows = GetBundleListsForRepo::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
        )
        .await?;

        // +----------------------+-------------+
        // | in_bundle_list_order | bundle_list |
        // +----------------------+-------------+
        // |                    1 |           3 |
        // |                    1 |           2 |
        // |                    2 |           2 |
        // |                    3 |           2 |
        // |                    1 |           1 |
        // +----------------------+-------------+
        let mut bundle_lists = vec![];
        let mut rows_iter = rows.into_iter().peekable();
        while let Some((
            handle,
            in_bundle_list_order,
            first_seen_bundle_list,
            fingerprint,
            generation_start_timestamp,
        )) = rows_iter.next()
        {
            let current_bundle_list_num = first_seen_bundle_list;
            // First bundle for a new bundle-list.
            let mut bundles = vec![Bundle {
                in_bundle_list_order,
                handle,
                fingerprint,
                generation_start_timestamp,
            }];

            // Rest of the bundles for the new bundle-list.
            while let Some((
                handle,
                in_bundle_list_order,
                bundle_list,
                fingerprint,
                generation_start_timestamp,
            )) = rows_iter.peek()
            {
                if current_bundle_list_num == *bundle_list {
                    // This bundle belongs to the current bundle-list.
                    bundles.push(Bundle {
                        in_bundle_list_order: *in_bundle_list_order,
                        handle: handle.clone(),
                        fingerprint: fingerprint.clone(),
                        generation_start_timestamp: generation_start_timestamp.clone(),
                    });
                    rows_iter.next();
                } else {
                    // This bundle is the first elem of the next bundle-list. Do not consume it.
                    // Break the loop to finish processing current bundle.
                    break;
                }
            }
            bundle_lists.push(BundleList {
                bundle_list_num: current_bundle_list_num,
                bundles,
            });
        }

        Ok(bundle_lists)
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use context::CoreContext;
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
                generation_start_timestamp: 0,
            },
            Bundle {
                in_bundle_list_order: 2,
                handle: String::from("handle2137.1.2"),
                fingerprint: String::from("fingerprint2137.1.2"),
                generation_start_timestamp: 0,
            },
        ];
        static ref TEST_BUNDLE_LIST_3: [Bundle; 3] = [
            Bundle {
                in_bundle_list_order: 1,
                handle: String::from("handle2137.2.1"),
                fingerprint: String::from("fingerprint2137.2.1"),
                generation_start_timestamp: 0,
            },
            Bundle {
                in_bundle_list_order: 2,
                handle: String::from("handle2137.2.2"),
                fingerprint: String::from("fingerprint2137.2.2"),
                generation_start_timestamp: 0,
            },
            Bundle {
                in_bundle_list_order: 3,
                handle: String::from("handle2137.2.3"),
                fingerprint: String::from("fingerprint2137.2.3"),
                generation_start_timestamp: 0,
            },
        ];
    }

    #[mononoke::fbinit_test]
    async fn test_no_bundles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        let bundle_list = storage.get_latest_bundle_list(&ctx).await?;
        assert!(bundle_list.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_add_bundles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_2[..])
            .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_latest_bundles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_2[..])
            .await?;

        let bundle_list = storage
            .get_latest_bundle_list(&ctx)
            .await?
            .expect("Should return bundle-list");
        assert_eq!(bundle_list.bundles.len(), 2);
        assert_eq!(bundle_list.bundle_list_num, 1);
        for (p, n) in bundle_list.bundles.iter().tuple_windows() {
            assert!(p.in_bundle_list_order < n.in_bundle_list_order)
        }

        storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_3[..])
            .await?;

        let bundle_list = storage
            .get_latest_bundle_list(&ctx)
            .await?
            .expect("Should return bundle-list");
        assert_eq!(bundle_list.bundles.len(), 3);
        assert_eq!(bundle_list.bundle_list_num, 2);
        for (p, n) in bundle_list.bundles.iter().tuple_windows() {
            assert!(p.in_bundle_list_order < n.in_bundle_list_order)
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_bundle_lists(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_2[..])
            .await?;
        storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_3[..])
            .await?;

        let bundle_lists = storage.get_bundle_lists(&ctx).await?;
        assert_eq!(bundle_lists.len(), 2);
        for bundle_list in bundle_lists.iter() {
            for (p, n) in bundle_list.bundles.iter().tuple_windows() {
                assert!(p.in_bundle_list_order < n.in_bundle_list_order)
            }
        }
        for (p, n) in bundle_lists.iter().tuple_windows() {
            assert!(p.bundle_list_num > n.bundle_list_num)
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_remove_bundle_list(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(2137);
        let storage = SqlGitBundleMetadataStorageBuilder::with_sqlite_in_memory()?.build(repo_id);

        let bundle_list_num_2 = storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_2[..])
            .await?;
        let bundle_list_num_3 = storage
            .add_new_bundles(&ctx, &TEST_BUNDLE_LIST_3[..])
            .await?;

        let bundle_lists = storage.get_bundle_lists(&ctx).await?;
        assert_eq!(bundle_lists.len(), 2);
        assert_eq!(bundle_lists[0].bundle_list_num, bundle_list_num_3);
        assert_eq!(bundle_lists[1].bundle_list_num, bundle_list_num_2);
        storage.remove_bundle_list(&ctx, bundle_list_num_2).await?;
        let bundle_lists = storage.get_bundle_lists(&ctx).await?;
        assert_eq!(bundle_lists.len(), 1);
        assert_eq!(bundle_lists[0].bundle_list_num, bundle_list_num_3);
        storage.remove_bundle_list(&ctx, bundle_list_num_3).await?;
        let bundle_lists = storage.get_bundle_lists(&ctx).await?;
        assert_eq!(bundle_lists.len(), 0);

        Ok(())
    }
}
