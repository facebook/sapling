/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests {
    use crate::{
        check_dependent_systems, check_repo_not_pushredirected, merge_imported_commit,
        move_bookmark, push_merge_commit, sort_bcs, ChangesetArgs, CheckerFlags,
        LATEST_REPLAYED_REQUEST_KEY,
    };

    use anyhow::Result;
    use blobstore::Loadable;
    use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
    use cached_config::{ConfigStore, TestSource};
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::{compat::Future01CompatExt, stream::TryStreamExt};
    use live_commit_sync_config::{
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS, CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS,
        CONFIGERATOR_PUSHREDIRECT_ENABLE,
    };
    use mercurial_types_mocks::nodehash::ONES_CSID as HG_CSID;
    use metaconfig_types::{PushrebaseParams, RepoConfig};
    use mononoke_types::{
        globalrev::{Globalrev, START_COMMIT_GLOBALREV},
        DateTime, RepositoryId,
    };
    use mononoke_types_mocks::changesetid::{ONES_CSID as MON_CSID, TWOS_CSID};
    use mutable_counters::{MutableCounters, SqlMutableCounters};
    use sql::rusqlite::Connection as SqliteConnection;
    use sql_construct::SqlConstruct;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tests_utils::{bookmark, drawdag::create_from_dag, CreateCommitContext};
    use tokio::time;

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book.to_string()).unwrap()
    }

    #[fbinit::compat_test]
    async fn test_move_bookmark(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo = blobrepo_factory::new_memblob_empty(None)?;
        let batch_size: usize = 2;
        let call_sign = Some("FBS");
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: true,
            call_sign,
        };
        let sleep_time = 1;
        let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();
        let changesets = create_from_dag(
            &ctx,
            &blob_repo,
            r##"
                A-B-C-D-E-F-G
            "##,
        )
        .await?;
        let mut bonsais = vec![];
        for (_, csid) in &changesets {
            bonsais.push(csid.load(ctx.clone(), &blob_repo.get_blobstore()).await?);
        }
        bonsais = sort_bcs(&bonsais)?;
        move_bookmark(
            &ctx,
            &blob_repo,
            &bonsais,
            batch_size,
            "test_repo",
            &checker_flags,
            sleep_time,
            &mutable_counters,
        )
        .await?;
        // Check the bookmark moves created BookmarkLogUpdate entries
        let entries = blob_repo
            .attribute_expected::<dyn BookmarkUpdateLog>()
            .list_bookmark_log_entries(
                ctx.clone(),
                BookmarkName::new("repo_import_test_repo")?,
                5,
                None,
                Freshness::MostRecent,
            )
            .map_ok(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
            .try_collect::<Vec<_>>()
            .await?;

        assert_eq!(
            entries,
            vec![
                (Some(changesets["G"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["F"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["D"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["B"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["A"]), BookmarkUpdateReason::ManualMove),
            ]
        );
        Ok(())
    }

    /*
                        largest_id      mutable_counters value   assert
        No action       None            None                     Error
        Move bookmark   1               None                     Error
        Set counter     1               1                        Ok(())
        Move bookmark   2               1                        inifite loop -> timeout
        Set counter     2               2                        Ok(())
    */
    #[fbinit::compat_test]
    async fn test_hg_sync_check(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: false,
            call_sign: None,
        };
        let sleep_time = 1;
        let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();
        let repo_id = repo.get_repoid();
        let bookmark = create_bookmark_name("book");

        assert!(check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters
        )
        .await
        .is_err());

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        txn.create(&bookmark, MON_CSID, BookmarkUpdateReason::TestMove, None)?;
        txn.commit().await.unwrap();
        assert!(check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters
        )
        .await
        .is_err());

        mutable_counters
            .set_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY, 1, None)
            .compat()
            .await?;

        check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters,
        )
        .await?;

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        txn.update(
            &bookmark,
            TWOS_CSID,
            MON_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )?;
        txn.commit().await.unwrap();

        let timed_out = time::timeout(
            Duration::from_millis(2000),
            check_dependent_systems(
                &ctx,
                &repo,
                &checker_flags,
                HG_CSID,
                sleep_time,
                &mutable_counters,
            ),
        )
        .await
        .is_err();
        assert!(timed_out);

        mutable_counters
            .set_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY, 2, None)
            .compat()
            .await?;

        check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters,
        )
        .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_merge_push_commit(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, _con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(1),
        )?;

        let master_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a", "a")
            .commit()
            .await?;
        let imported_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("b", "b")
            .commit()
            .await?;
        let imported_cs = imported_cs_id.load(ctx.clone(), repo.blobstore()).await?;

        let dest_bookmark = bookmark(&ctx, &repo, "master").set_to(master_cs_id).await?;

        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "merging".to_string(),
            datetime: DateTime::now(),
        };

        let merged_cs_id = merge_imported_commit(
            &ctx,
            &repo,
            &[imported_cs.clone()],
            &dest_bookmark,
            changeset_args,
        )
        .await?;

        let mut repo_config = RepoConfig::default();
        repo_config.pushrebase = PushrebaseParams {
            assign_globalrevs: true,
            ..Default::default()
        };

        let pushed_cs_id =
            push_merge_commit(&ctx, &repo, merged_cs_id, &dest_bookmark, &repo_config).await?;
        let pushed_cs = pushed_cs_id.load(ctx.clone(), repo.blobstore()).await?;

        assert_eq!(
            Globalrev::new(START_COMMIT_GLOBALREV),
            Globalrev::from_bcs(&pushed_cs)?
        );
        Ok(())
    }

    const PUSHREDIRECTOR_PUBLIC_ENABLED: &str = r#"{
        "per_repo": {
            "1": {
                "draft_push": false,
                "public_push": true
            },
            "2": {
                "draft_push": true,
                "public_push": false
            }
        }
    }"#;

    const CURRENT_COMMIT_SYNC_CONFIG: &str = r#"{
        "repos": {
            "large_repo_1": {
                "large_repo_id": 0,
                "common_pushrebase_bookmarks": ["b1"],
                "small_repos": [
                    {
                        "repoid": 1,
                        "default_action": "prepend_prefix",
                        "default_prefix": "f1",
                        "bookmark_prefix": "bp1/",
                        "mapping": {"d": "dd"},
                        "direction": "large_to_small"
                    },
                    {
                        "repoid": 2,
                        "default_action": "prepend_prefix",
                        "default_prefix": "f2",
                        "bookmark_prefix": "bp2/",
                        "mapping": {"d": "ddd"},
                        "direction": "small_to_large"
                    }
                ],
                "version_name": "TEST_VERSION_NAME_LIVE_1"
            }
        }
    }"#;

    const EMTPY_COMMMIT_SYNC_ALL: &str = r#"{
        "repos": {}
    }"#;

    fn insert_repo_config(id: i32, repos: &mut HashMap<String, RepoConfig>) {
        let repo_config = RepoConfig {
            repoid: RepositoryId::new(id),
            ..Default::default()
        };
        repos.insert(format!("repo{}", id), repo_config);
    }

    /*
        repo0: no push-redirection => pass
        repo1: push-redirects to repo0 => error: "The destination repo..."
        repo2: draft push-redirects => pass
    */
    #[fbinit::compat_test]
    async fn test_check_repo_not_pushredirected(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let test_source = Arc::new(TestSource::new());

        let mut repos: HashMap<String, RepoConfig> = HashMap::new();

        test_source.insert_config(
            CONFIGERATOR_PUSHREDIRECT_ENABLE,
            PUSHREDIRECTOR_PUBLIC_ENABLED,
            0,
        );

        test_source.insert_config(
            CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS,
            CURRENT_COMMIT_SYNC_CONFIG,
            0,
        );

        test_source.insert_config(
            CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
            EMTPY_COMMMIT_SYNC_ALL,
            0,
        );

        test_source.insert_to_refresh(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string());
        test_source.insert_to_refresh(CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS.to_string());
        test_source.insert_to_refresh(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string());

        let config_store = ConfigStore::new(test_source.clone(), Duration::from_millis(2), None);
        let maybe_config_store = Some(config_store);

        let (repo0, _) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(0),
        )?;

        insert_repo_config(0, &mut repos);
        check_repo_not_pushredirected(&ctx, &repo0, &maybe_config_store, &repos).await?;

        let (repo1, _) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(1),
        )?;

        insert_repo_config(1, &mut repos);
        assert!(
            check_repo_not_pushredirected(&ctx, &repo1, &maybe_config_store, &repos)
                .await
                .is_err()
        );

        let (repo2, _) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(2),
        )?;

        insert_repo_config(2, &mut repos);
        check_repo_not_pushredirected(&ctx, &repo2, &maybe_config_store, &repos).await?;
        Ok(())
    }
}
