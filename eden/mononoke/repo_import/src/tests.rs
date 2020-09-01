/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests {
    use crate::{
        check_dependent_systems, get_large_repo_config_if_pushredirected, get_large_repo_setting,
        merge_imported_commit, move_bookmark, push_merge_commit, sort_bcs, ChangesetArgs,
        CheckerFlags, RepoImportSetting, LATEST_REPLAYED_REQUEST_KEY,
    };

    use anyhow::Result;
    use ascii::AsciiString;
    use blobrepo::BlobRepo;
    use blobstore::Loadable;
    use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
    use cached_config::{ConfigStore, TestSource};
    use context::CoreContext;
    use cross_repo_sync::create_commit_syncers;
    use fbinit::FacebookInit;
    use futures::{compat::Future01CompatExt, stream::TryStreamExt};
    use live_commit_sync_config::{
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS, CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS,
        CONFIGERATOR_PUSHREDIRECT_ENABLE,
    };
    use maplit::hashmap;
    use mercurial_types::MPath;
    use mercurial_types_mocks::nodehash::ONES_CSID as HG_CSID;
    use metaconfig_types::{
        CommitSyncConfig, CommitSyncConfigVersion, CommitSyncDirection,
        DefaultSmallToLargeCommitSyncPathAction, PushrebaseParams, RepoConfig,
        SmallRepoCommitSyncConfig,
    };
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
    use synced_commit_mapping::SqlSyncedCommitMapping;
    use tests_utils::{bookmark, drawdag::create_from_dag, CreateCommitContext};
    use tokio::time;

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book.to_string()).unwrap()
    }

    fn mp(s: &'static str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn create_repo(id: i32) -> Result<BlobRepo> {
        let (repo, _) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(id),
        )?;
        Ok(repo)
    }
    #[fbinit::compat_test]
    async fn test_move_bookmark(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo = blobrepo_factory::new_memblob_empty(None)?;
        let batch_size: usize = 2;
        let call_sign = Some("FBS".to_string());
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
        let importing_bookmark = BookmarkName::new("repo_import_test_repo")?;
        move_bookmark(
            &ctx,
            &blob_repo,
            &bonsais,
            batch_size,
            &importing_bookmark,
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
        let repo = create_repo(1)?;

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
        repo0: no push-redirection => None
        repo1: push-redirects to repo0 => Some(repo_config)
        repo2: draft push-redirects => None
    */
    #[fbinit::compat_test]
    async fn test_get_large_repo_config_if_pushredirected(fb: FacebookInit) -> Result<()> {
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

        let repo0 = create_repo(0)?;

        insert_repo_config(0, &mut repos);
        assert!(
            get_large_repo_config_if_pushredirected(&ctx, &repo0, &maybe_config_store, &repos)
                .await?
                .is_none()
        );

        let repo1 = create_repo(1)?;

        insert_repo_config(1, &mut repos);
        get_large_repo_config_if_pushredirected(&ctx, &repo1, &maybe_config_store, &repos).await?;

        let repo2 = create_repo(2)?;

        insert_repo_config(2, &mut repos);
        assert!(
            get_large_repo_config_if_pushredirected(&ctx, &repo2, &maybe_config_store, &repos)
                .await?
                .is_none()
        );
        Ok(())
    }

    fn get_small_repo_sync_config_1() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp(
                "large_repo",
            )),
            map: HashMap::new(),
            bookmark_prefix: AsciiString::from_ascii("large_repo_bookmark/".to_string()).unwrap(),
            direction: CommitSyncDirection::SmallToLarge,
        }
    }

    fn get_small_repo_sync_config_2() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
            map: hashmap! {
                mp("dest_path_prefix_2") => mp("dpp2"),
            },
            bookmark_prefix: AsciiString::from_ascii("large_repo_bookmark_2/".to_string()).unwrap(),
            direction: CommitSyncDirection::SmallToLarge,
        }
    }

    fn get_large_repo_sync_config() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(0),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_1(),
                RepositoryId::new(2) => get_small_repo_sync_config_2(),
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION".to_string()),
        }
    }

    /*
        The test checks, if get_large_repo_setting function returns the correct
        variables for a large repo setting given small repo settings.
        -> bookmarks are prepended with the bookmark_prefixes given in the configs
    */
    #[fbinit::compat_test]
    async fn test_get_large_repo_setting(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        let large_repo = create_repo(0)?;
        let small_repo_1 = create_repo(1)?;

        let small_repo_setting_1 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("importing_bookmark"),
            dest_bookmark: create_bookmark_name("dest_bookmark"),
        };

        let syncers_1 = create_commit_syncers(
            small_repo_1.clone(),
            large_repo.clone(),
            &get_large_repo_sync_config(),
            mapping.clone(),
        )?;

        let large_repo_setting_1 =
            get_large_repo_setting(&ctx, &small_repo_setting_1, &syncers_1.small_to_large).await?;

        let expected_large_repo_setting_1 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("large_repo_bookmark/importing_bookmark"),
            dest_bookmark: create_bookmark_name("large_repo_bookmark/dest_bookmark"),
        };

        assert_eq!(expected_large_repo_setting_1, large_repo_setting_1);

        let small_repo_2 = create_repo(2)?;

        let small_repo_setting_2 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("importing_bookmark_2"),
            dest_bookmark: create_bookmark_name("dest_bookmark_2"),
        };

        let syncers_2 = create_commit_syncers(
            small_repo_2.clone(),
            large_repo.clone(),
            &get_large_repo_sync_config(),
            mapping.clone(),
        )?;

        let large_repo_setting_2 =
            get_large_repo_setting(&ctx, &small_repo_setting_2, &syncers_2.small_to_large).await?;

        let expected_large_repo_setting_2 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("large_repo_bookmark_2/importing_bookmark_2"),
            dest_bookmark: create_bookmark_name("large_repo_bookmark_2/dest_bookmark_2"),
        };

        assert_eq!(expected_large_repo_setting_2, large_repo_setting_2);
        Ok(())
    }
}
