/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests {
    use crate::back_sync_commits_to_small_repo;
    use crate::check_dependent_systems;
    use crate::derive_bonsais_single_repo;
    use crate::find_mapping_version;
    use crate::get_large_repo_config_if_pushredirected;
    use crate::get_large_repo_setting;
    use crate::merge_imported_commit;
    use crate::move_bookmark;
    use crate::push_merge_commit;
    use crate::rewrite_file_paths;
    use crate::ChangesetArgs;
    use crate::CheckerFlags;
    use crate::ImportStage;
    use crate::RecoveryFields;
    use crate::Repo;
    use crate::RepoImportSetting;
    use anyhow::Result;
    use ascii::AsciiString;
    use blobrepo::AsBlobRepo;
    use blobstore::Loadable;
    use bookmarks::BookmarkName;
    use bookmarks::BookmarkUpdateLogRef;
    use bookmarks::BookmarkUpdateReason;
    use bookmarks::BookmarksRef;
    use bookmarks::Freshness;
    use cacheblob::InProcessLease;
    use cached_config::ConfigStore;
    use cached_config::ModificationTime;
    use cached_config::TestSource;
    use context::CoreContext;
    use cross_repo_sync::create_commit_syncers;
    use cross_repo_sync::CommitSyncContext;
    use derived_data_manager::BonsaiDerivable;
    use derived_data_utils::derived_data_utils;
    use fbinit::FacebookInit;
    use futures::stream::TryStreamExt;
    use git_types::TreeHandle;
    use live_commit_sync_config::CfgrLiveCommitSyncConfig;
    use live_commit_sync_config::LiveCommitSyncConfig;
    use live_commit_sync_config::TestLiveCommitSyncConfig;
    use live_commit_sync_config::CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS;
    use live_commit_sync_config::CONFIGERATOR_PUSHREDIRECT_ENABLE;
    use maplit::hashmap;
    use mercurial_types::MPath;
    use mercurial_types_mocks::nodehash::ONES_CSID as HG_CSID;
    use metaconfig_types::CommitSyncConfig;
    use metaconfig_types::CommitSyncConfigVersion;
    use metaconfig_types::CommonCommitSyncConfig;
    use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
    use metaconfig_types::PushrebaseParams;
    use metaconfig_types::RepoConfig;
    use metaconfig_types::SmallRepoCommitSyncConfig;
    use metaconfig_types::SmallRepoPermanentConfig;
    use mononoke_hg_sync_job_helper_lib::LATEST_REPLAYED_REQUEST_KEY;
    use mononoke_types::globalrev::Globalrev;
    use mononoke_types::globalrev::START_COMMIT_GLOBALREV;
    use mononoke_types::BonsaiChangeset;
    use mononoke_types::ChangesetId;
    use mononoke_types::DateTime;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::ONES_CSID as MON_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use movers::DefaultAction;
    use movers::Mover;
    use mutable_counters::MutableCountersRef;
    use repo_blobstore::RepoBlobstoreRef;
    use sql_construct::SqlConstruct;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;
    use synced_commit_mapping::SqlSyncedCommitMapping;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::bookmark;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;
    use tokio::time;

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book).unwrap()
    }

    fn mp(s: &'static str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn create_repo(fb: FacebookInit, id: i32) -> Result<Repo> {
        let repo: Repo = TestRepoFactory::new(fb)?
            .with_config_override(|config| {
                config
                    .derived_data_config
                    .get_active_config()
                    .expect("No enabled derived data types config")
                    .types
                    .remove(TreeHandle::NAME);
            })
            .with_id(RepositoryId::new(id))
            .build()?;
        Ok(repo)
    }

    fn get_file_changes_mpaths(bcs: &BonsaiChangeset) -> Vec<MPath> {
        bcs.file_changes()
            .map(|(mpath, _)| mpath)
            .cloned()
            .collect()
    }

    fn create_mock_recovery_fields() -> RecoveryFields {
        RecoveryFields {
            import_stage: ImportStage::GitImport,
            recovery_file_path: "recovery_path".to_string(),
            git_repo_path: "git_repo_path".to_string(),
            git_merge_bcs_id: None,
            git_merge_rev_id: "master".to_string(),
            dest_path: "dest_path".to_string(),
            bookmark_suffix: "bookmark_suffix".to_string(),
            batch_size: 2,
            move_bookmark_commits_done: 0,
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: true,
            sleep_time: Duration::from_secs(1),
            dest_bookmark_name: "dest_bookmark_name".to_string(),
            commit_author: "commit_author".to_string(),
            commit_message: "commit_message".to_string(),
            datetime: DateTime::now(),
            imported_cs_id: None,
            shifted_bcs_ids: None,
            gitimport_bcs_ids: None,
            merged_cs_id: None,
        }
    }

    #[fbinit::test]
    async fn test_move_bookmark(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(fb)?;
        let mut recovery_fields = create_mock_recovery_fields();
        let call_sign = Some("FBS".to_string());
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: true,
        };
        let changesets = create_from_dag(
            &ctx,
            repo.as_blob_repo(),
            r##"
                A-B-C-D-E-F-G
            "##,
        )
        .await?;

        let bcs_ids: Vec<ChangesetId> = changesets.values().copied().collect();
        let importing_bookmark = BookmarkName::new("repo_import_test_repo")?;
        move_bookmark(
            &ctx,
            &repo,
            &bcs_ids,
            &importing_bookmark,
            &checker_flags,
            &call_sign,
            &None,
            &mut recovery_fields,
        )
        .await?;
        // Check the bookmark moves created BookmarkLogUpdate entries
        let entries = repo
            .bookmark_update_log()
            .list_bookmark_log_entries(
                ctx.clone(),
                BookmarkName::new("repo_import_test_repo")?,
                5,
                None,
                Freshness::MostRecent,
            )
            .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
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

    #[fbinit::test]
    async fn test_move_bookmark_with_existing_bookmark(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(fb)?;
        let mut recovery_fields = create_mock_recovery_fields();
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: true,
        };
        let changesets = create_from_dag(
            &ctx,
            repo.as_blob_repo(),
            r##"
                A-B-C-D-E-F-G
            "##,
        )
        .await?;

        let bcs_ids: Vec<ChangesetId> = changesets.values().copied().collect();
        let importing_bookmark = BookmarkName::new("repo_import_test_repo")?;
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.create(
            &importing_bookmark,
            bcs_ids.first().unwrap().clone(),
            BookmarkUpdateReason::ManualMove,
        )?;
        txn.commit().await.unwrap();
        move_bookmark(
            &ctx,
            &repo,
            &bcs_ids,
            &importing_bookmark,
            &checker_flags,
            &None,
            &None,
            &mut recovery_fields,
        )
        .await?;
        // Check the bookmark moves created BookmarkLogUpdate entries
        let entries = repo
            .bookmark_update_log()
            .list_bookmark_log_entries(
                ctx.clone(),
                BookmarkName::new("repo_import_test_repo")?,
                5,
                None,
                Freshness::MostRecent,
            )
            .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
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
    #[fbinit::test]
    async fn test_hg_sync_check(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(fb)?;
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: false,
        };
        let call_sign = None;
        let sleep_time = Duration::from_secs(1);
        let bookmark = create_bookmark_name("book");

        assert!(
            check_dependent_systems(&ctx, &repo, &checker_flags, HG_CSID, sleep_time, &call_sign,)
                .await
                .is_err()
        );

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.create(&bookmark, MON_CSID, BookmarkUpdateReason::TestMove)?;
        txn.commit().await.unwrap();
        assert!(
            check_dependent_systems(&ctx, &repo, &checker_flags, HG_CSID, sleep_time, &call_sign,)
                .await
                .is_err()
        );

        repo.mutable_counters()
            .set_counter(&ctx, LATEST_REPLAYED_REQUEST_KEY, 1, None)
            .await?;

        check_dependent_systems(&ctx, &repo, &checker_flags, HG_CSID, sleep_time, &call_sign)
            .await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.update(
            &bookmark,
            TWOS_CSID,
            MON_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.commit().await.unwrap();

        let timed_out = time::timeout(
            Duration::from_millis(2000),
            check_dependent_systems(&ctx, &repo, &checker_flags, HG_CSID, sleep_time, &call_sign),
        )
        .await
        .is_err();
        assert!(timed_out);

        repo.mutable_counters()
            .set_counter(&ctx, LATEST_REPLAYED_REQUEST_KEY, 2, None)
            .await?;

        check_dependent_systems(&ctx, &repo, &checker_flags, HG_CSID, sleep_time, &call_sign)
            .await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_merge_push_commit(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = create_repo(fb, 1)?;

        let master_cs_id = CreateCommitContext::new_root(&ctx, repo.as_blob_repo())
            .add_file("a", "a")
            .commit()
            .await?;
        let imported_cs_id = CreateCommitContext::new_root(&ctx, repo.as_blob_repo())
            .add_file("b", "b")
            .commit()
            .await?;

        let dest_bookmark = bookmark(&ctx, repo.as_blob_repo(), "master")
            .set_to(master_cs_id)
            .await?;

        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "merging".to_string(),
            datetime: DateTime::now(),
        };

        let merged_cs_id =
            merge_imported_commit(&ctx, &repo, imported_cs_id, &dest_bookmark, changeset_args)
                .await?;

        let mut repo_config = RepoConfig::default();
        repo_config.pushrebase = PushrebaseParams {
            globalrevs_publishing_bookmark: Some(BookmarkName::new("master")?),
            ..Default::default()
        };

        let pushed_cs_id =
            push_merge_commit(&ctx, &repo, merged_cs_id, &dest_bookmark, &repo_config).await?;
        let pushed_cs = pushed_cs_id.load(&ctx, repo.repo_blobstore()).await?;

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

    const COMMIT_SYNC_ALL: &str = r#"{
        "repos": {
            "large_repo_1": {
                "versions": [{
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
                }],
                "current_version": "TEST_VERSION_NAME_LIVE_1",
                "common": {
                    "common_pushrebase_bookmarks": ["b1"],
                    "large_repo_id": 0,
                    "small_repos": {
                      "1": {
                        "bookmark_prefix": "bp1/"
                      },
                      "2": {
                        "bookmark_prefix": "bp2/"
                      }
                    }
                }
            }
        }
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
    #[fbinit::test]
    async fn test_get_large_repo_config_if_pushredirected(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let test_source = Arc::new(TestSource::new());

        let mut repos: HashMap<String, RepoConfig> = HashMap::new();

        test_source.insert_config(
            CONFIGERATOR_PUSHREDIRECT_ENABLE,
            PUSHREDIRECTOR_PUBLIC_ENABLED,
            ModificationTime::UnixTimestamp(0),
        );

        test_source.insert_config(
            CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
            COMMIT_SYNC_ALL,
            ModificationTime::UnixTimestamp(0),
        );

        test_source.insert_to_refresh(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string());
        test_source.insert_to_refresh(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string());

        let config_store = ConfigStore::new(test_source.clone(), Duration::from_millis(2), None);
        let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;

        let repo0 = create_repo(fb, 0)?;

        insert_repo_config(0, &mut repos);
        assert!(
            get_large_repo_config_if_pushredirected(&repo0, &live_commit_sync_config, &repos)
                .await?
                .is_none()
        );

        let repo1 = create_repo(fb, 1)?;

        insert_repo_config(1, &mut repos);
        assert!(
            get_large_repo_config_if_pushredirected(&repo1, &live_commit_sync_config, &repos)
                .await?
                .is_some()
        );

        let repo2 = create_repo(fb, 2)?;

        insert_repo_config(2, &mut repos);
        assert!(
            get_large_repo_config_if_pushredirected(&repo2, &live_commit_sync_config, &repos)
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
            map: hashmap! {
                mp("dest_path_prefix/B") => mp("random_dir/B"),
            },
        }
    }

    fn get_small_repo_sync_config_1_later() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp(
                "large_repo",
            )),
            map: hashmap! {
                mp("dest_path_prefix/B") => mp("random_dir/B"),
                mp("dest_path_prefix/C") => mp("random_dir/C"),
            },
        }
    }

    fn get_small_repo_sync_config_2() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
            map: hashmap! {
                mp("dest_path_prefix_2") => mp("dpp2"),
            },
        }
    }

    fn get_large_repo_live_commit_sync_config() -> Arc<dyn LiveCommitSyncConfig> {
        let commit_sync_config = get_large_repo_sync_config();
        let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();

        let later_commit_sync_config = get_later_large_repo_sync_config();

        source.add_config(commit_sync_config.clone());
        source.add_config(later_commit_sync_config);
        source.add_common_config(CommonCommitSyncConfig {
            common_pushrebase_bookmarks: commit_sync_config.common_pushrebase_bookmarks.clone(),
            small_repos: hashmap! {
                RepositoryId::new(1) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("large_repo_bookmark/")
                        .unwrap(),
                },
                RepositoryId::new(2) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("large_repo_bookmark_2/")
                        .unwrap(),
                },
            },
            large_repo_id: commit_sync_config.large_repo_id,
        });

        Arc::new(sync_config)
    }

    fn first_version() -> CommitSyncConfigVersion {
        CommitSyncConfigVersion("TEST_VERSION".to_string())
    }

    fn get_large_repo_sync_config() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(0),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_1(),
                RepositoryId::new(2) => get_small_repo_sync_config_2(),
            },
            version_name: first_version(),
        }
    }

    fn get_later_large_repo_sync_config() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(0),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_1_later(),
                RepositoryId::new(2) => get_small_repo_sync_config_2(),
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION2".to_string()),
        }
    }

    /*
        The test checks, if get_large_repo_setting function returns the correct
        variables for a large repo setting given small repo settings.
        -> bookmarks are prepended with the bookmark_prefixes given in the configs
    */
    #[fbinit::test]
    async fn test_get_large_repo_setting(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        let large_repo = create_repo(fb, 0)?;
        let small_repo_1 = create_repo(fb, 1)?;

        let small_repo_setting_1 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("importing_bookmark"),
            dest_bookmark: create_bookmark_name("dest_bookmark"),
        };

        let live_commit_sync_config = get_large_repo_live_commit_sync_config();
        let syncers_1 = create_commit_syncers(
            &ctx,
            small_repo_1.as_blob_repo().clone(),
            large_repo.as_blob_repo().clone(),
            mapping.clone(),
            live_commit_sync_config.clone(),
            Arc::new(InProcessLease::new()),
        )?;

        let large_repo_setting_1 =
            get_large_repo_setting(&ctx, &small_repo_setting_1, &syncers_1.small_to_large).await?;

        let expected_large_repo_setting_1 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("large_repo_bookmark/importing_bookmark"),
            dest_bookmark: create_bookmark_name("large_repo_bookmark/dest_bookmark"),
        };

        assert_eq!(expected_large_repo_setting_1, large_repo_setting_1);

        let small_repo_2 = create_repo(fb, 2)?;

        let small_repo_setting_2 = RepoImportSetting {
            importing_bookmark: create_bookmark_name("importing_bookmark_2"),
            dest_bookmark: create_bookmark_name("dest_bookmark_2"),
        };

        let syncers_2 = create_commit_syncers(
            &ctx,
            small_repo_2.as_blob_repo().clone(),
            large_repo.as_blob_repo().clone(),
            mapping.clone(),
            live_commit_sync_config,
            Arc::new(InProcessLease::new()),
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

    /*
        The test checks if we have imported the files into the right paths
        in a pushredirected large repo and if we have back-sycned the files
        in the right places in small repo.

        Given file A, B and destination prefix "dest_path_prefix".
        If we imported into small repo and allowed it to push-redirect to
        large repo, we would get the following path rewriting sequences:
        A -> dest_path_prefix/A (in small_repo) -> large_repo/dest_path_prefix/A
        B -> dest_path_prefix/B (in small_repo) -> random_dir/B

        Therefore, repo_import tool should import A into large_repo_dest_prefix/A
        and B into random_dir/B places. When we backsync to small_repo, we should get
        dest_path_prefix/A and dest_path_prefix/B paths, respectively.
    */
    #[fbinit::test]
    async fn test_rewrite_file_paths_and_backsync(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let large_repo = create_repo(fb, 0)?;
        let small_repo = create_repo(fb, 1)?;
        let changesets = create_from_dag(
            &ctx,
            large_repo.as_blob_repo(),
            r##"
                A-B
            "##,
        )
        .await?;
        let cs_ids: Vec<ChangesetId> = changesets.values().copied().collect();

        let live_commit_sync_config = get_large_repo_live_commit_sync_config();
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        let syncers = create_commit_syncers(
            &ctx,
            small_repo.as_blob_repo().clone(),
            large_repo.as_blob_repo().clone(),
            mapping.clone(),
            live_commit_sync_config,
            Arc::new(InProcessLease::new()),
        )?;

        let large_to_small_syncer = syncers.large_to_small;
        let mut movers = vec![];
        let importing_mover = movers::mover_factory(
            HashMap::new(),
            DefaultAction::PrependPrefix(mp("dest_path_prefix")),
        )?;
        movers.push(importing_mover);
        movers.push(
            syncers
                .small_to_large
                .get_mover_by_version(&CommitSyncConfigVersion("TEST_VERSION".to_string()))
                .await?,
        );

        let combined_mover: Mover = Arc::new(move |source_path: &MPath| {
            let mut mutable_path = source_path.clone();
            for mover in movers.clone() {
                let maybe_path = mover(&mutable_path)?;
                mutable_path = match maybe_path {
                    Some(moved_path) => moved_path,
                    None => return Ok(None),
                };
            }
            Ok(Some(mutable_path))
        });

        let (shifted_bcs_ids, _git_merge_shifted_bcs_id) = rewrite_file_paths(
            &ctx,
            &large_repo,
            &combined_mover,
            &cs_ids,
            cs_ids.last().unwrap(),
        )
        .await?;

        let large_repo_cs_a = &shifted_bcs_ids[0]
            .load(&ctx, large_repo.repo_blobstore())
            .await?;
        let large_repo_cs_a_mpaths = get_file_changes_mpaths(large_repo_cs_a);
        assert_eq!(
            vec![mp("large_repo/dest_path_prefix/A")],
            large_repo_cs_a_mpaths
        );

        let large_repo_cs_b = &shifted_bcs_ids[1]
            .load(&ctx, large_repo.repo_blobstore())
            .await?;
        let large_repo_cs_b_mpaths = get_file_changes_mpaths(large_repo_cs_b);
        assert_eq!(vec![mp("random_dir/B")], large_repo_cs_b_mpaths);

        let synced_bcs_ids = back_sync_commits_to_small_repo(
            &ctx,
            &small_repo,
            &large_to_small_syncer,
            &shifted_bcs_ids,
            &first_version(),
        )
        .await?;

        let small_repo_cs_a = &synced_bcs_ids[0]
            .load(&ctx, small_repo.repo_blobstore())
            .await?;
        let small_repo_cs_a_mpaths = get_file_changes_mpaths(small_repo_cs_a);
        assert_eq!(vec![mp("dest_path_prefix/A")], small_repo_cs_a_mpaths);

        let small_repo_cs_b = &synced_bcs_ids[1]
            .load(&ctx, small_repo.repo_blobstore())
            .await?;
        let small_repo_cs_b_mpaths = get_file_changes_mpaths(small_repo_cs_b);
        assert_eq!(vec![mp("dest_path_prefix/B")], small_repo_cs_b_mpaths);

        Ok(())
    }

    async fn check_no_pending_commits(
        ctx: &CoreContext,
        repo: &Repo,
        cs_ids: &[ChangesetId],
    ) -> Result<()> {
        let blob_repo = repo.as_blob_repo();
        let derived_data_types = &blob_repo.get_active_derived_data_types_config().types;

        for derived_data_type in derived_data_types {
            let derived_utils = derived_data_utils(ctx.fb, blob_repo, derived_data_type)?;
            let pending = derived_utils
                .pending(ctx.clone(), repo.as_blob_repo().clone(), cs_ids.to_vec())
                .await?;
            assert!(pending.is_empty());
        }

        Ok(())
    }

    /*
        Given two repos and their changesets, we check if we have derived all the
        data types for the changesets
    */
    #[fbinit::test]
    async fn test_derive_bonsais_multiple_repos(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_0 = create_repo(fb, 0)?;

        let repo_0_commits = create_from_dag(
            &ctx,
            repo_0.as_blob_repo(),
            r##"
                A-B
            "##,
        )
        .await?;

        let repo_0_cs_ids: Vec<ChangesetId> = repo_0_commits.values().copied().collect();

        let repo_1 = create_repo(fb, 1)?;
        let repo_1_commits = create_from_dag(
            &ctx,
            repo_1.as_blob_repo(),
            r##"
                C-D
            "##,
        )
        .await?;

        let repo_1_cs_ids: Vec<ChangesetId> = repo_1_commits.values().copied().collect();

        derive_bonsais_single_repo(&ctx, &repo_0, &repo_0_cs_ids).await?;
        derive_bonsais_single_repo(&ctx, &repo_1, &repo_1_cs_ids).await?;

        check_no_pending_commits(&ctx, &repo_0, &repo_0_cs_ids).await?;
        check_no_pending_commits(&ctx, &repo_1, &repo_1_cs_ids).await?;
        Ok(())
    }

    /*
        The test combines rewrite and derive functionalities:
        Given a large repo that backsyncs to small_repo, we check if we have
        derived all the data types for both repos.
    */
    #[fbinit::test]
    async fn test_rewrite_and_derive(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let large_repo = create_repo(fb, 0)?;
        let small_repo = create_repo(fb, 1)?;
        let changesets = create_from_dag(
            &ctx,
            large_repo.as_blob_repo(),
            r##"
                A-B
            "##,
        )
        .await?;

        let cs_ids: Vec<ChangesetId> = changesets.values().copied().collect();

        let live_commit_sync_config = get_large_repo_live_commit_sync_config();
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
        let syncers = create_commit_syncers(
            &ctx,
            small_repo.as_blob_repo().clone(),
            large_repo.as_blob_repo().clone(),
            mapping.clone(),
            live_commit_sync_config,
            Arc::new(InProcessLease::new()),
        )?;

        let large_to_small_syncer = syncers.large_to_small;
        let mut movers = vec![];
        let importing_mover = movers::mover_factory(
            HashMap::new(),
            DefaultAction::PrependPrefix(mp("dest_path_prefix")),
        )?;
        movers.push(importing_mover);
        movers.push(
            syncers
                .small_to_large
                .get_mover_by_version(&CommitSyncConfigVersion("TEST_VERSION".to_string()))
                .await?,
        );

        let combined_mover: Mover = Arc::new(move |source_path: &MPath| {
            let mut mutable_path = source_path.clone();
            for mover in movers.clone() {
                let maybe_path = mover(&mutable_path)?;
                mutable_path = match maybe_path {
                    Some(moved_path) => moved_path,
                    None => return Ok(None),
                };
            }
            Ok(Some(mutable_path))
        });

        let (large_repo_cs_ids, _) = rewrite_file_paths(
            &ctx,
            &large_repo,
            &combined_mover,
            &cs_ids,
            cs_ids.last().unwrap(),
        )
        .await?;
        let small_repo_cs_ids = back_sync_commits_to_small_repo(
            &ctx,
            &small_repo,
            &large_to_small_syncer,
            &large_repo_cs_ids,
            &first_version(),
        )
        .await?;

        derive_bonsais_single_repo(&ctx, &large_repo, &large_repo_cs_ids).await?;
        derive_bonsais_single_repo(&ctx, &small_repo, &small_repo_cs_ids).await?;

        check_no_pending_commits(&ctx, &large_repo, &large_repo_cs_ids).await?;
        check_no_pending_commits(&ctx, &small_repo, &small_repo_cs_ids).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_find_version_and_backsync(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let large_repo = create_repo(fb, 0)?;
        let small_repo = create_repo(fb, 1)?;

        let root = CreateCommitContext::new_root(&ctx, large_repo.as_blob_repo())
            .add_file("random_dir/B/file", "text")
            .commit()
            .await?;

        let first_commit = CreateCommitContext::new(&ctx, large_repo.as_blob_repo(), vec![root])
            .add_file("large_repo/justfile", "justtext")
            .commit()
            .await?;

        bookmark(&ctx, large_repo.as_blob_repo(), "before_mapping_change")
            .set_to(first_commit)
            .await?;

        let live_commit_sync_config = get_large_repo_live_commit_sync_config();
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
        let syncers = create_commit_syncers(
            &ctx,
            small_repo.as_blob_repo().clone(),
            large_repo.as_blob_repo().clone(),
            mapping.clone(),
            live_commit_sync_config,
            Arc::new(InProcessLease::new()),
        )?;

        let large_to_small_syncer = syncers.large_to_small;

        let small_repo_cs_ids = back_sync_commits_to_small_repo(
            &ctx,
            &small_repo,
            &large_to_small_syncer,
            &[root, first_commit],
            &first_version(),
        )
        .await?;

        let wc =
            list_working_copy_utf8(&ctx, small_repo.as_blob_repo(), small_repo_cs_ids[0]).await?;
        assert_eq!(
            wc,
            hashmap! {
                MPath::new("dest_path_prefix/B/file")? => "text".to_string()
            }
        );

        let wc =
            list_working_copy_utf8(&ctx, small_repo.as_blob_repo(), small_repo_cs_ids[1]).await?;
        assert_eq!(
            wc,
            hashmap! {
                MPath::new("dest_path_prefix/B/file")? => "text".to_string(),
                MPath::new("justfile")? => "justtext".to_string(),
            }
        );

        // Change mapping
        let change_mapping_cs_id =
            CreateCommitContext::new(&ctx, large_repo.as_blob_repo(), vec![first_commit])
                .commit()
                .await?;
        bookmark(&ctx, large_repo.as_blob_repo(), "after_mapping_change")
            .set_to(change_mapping_cs_id)
            .await?;

        large_to_small_syncer
            .unsafe_always_rewrite_sync_commit(
                &ctx,
                change_mapping_cs_id,
                None,
                &CommitSyncConfigVersion("TEST_VERSION2".to_string()),
                CommitSyncContext::Tests,
            )
            .await?;

        assert_eq!(
            find_mapping_version(
                &ctx,
                &large_to_small_syncer,
                &BookmarkName::new("before_mapping_change")?,
            )
            .await?,
            Some(CommitSyncConfigVersion("TEST_VERSION".to_string()))
        );

        assert_eq!(
            find_mapping_version(
                &ctx,
                &large_to_small_syncer,
                &BookmarkName::new("after_mapping_change")?,
            )
            .await?,
            Some(CommitSyncConfigVersion("TEST_VERSION2".to_string()))
        );

        Ok(())
    }
}
