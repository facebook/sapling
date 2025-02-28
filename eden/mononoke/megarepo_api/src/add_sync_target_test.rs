/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use blobstore::Loadable;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::btreemap;
use maplit::hashmap;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Target;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::SourceName;
use megarepo_mapping::REMAPPING_STATE_FILE;
use metaconfig_types::RepoConfigArc;
use mononoke_macros::mononoke;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use tests_utils::bookmark;
use tests_utils::list_working_copy_utf8;
use tests_utils::list_working_copy_utf8_with_types;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;

use crate::add_sync_target::AddSyncTarget;
use crate::common::MegarepoOp;
use crate::common::SYNC_TARGET_CONFIG_FILE;
use crate::megarepo_test_utils::MegarepoTest;
use crate::megarepo_test_utils::SyncTargetConfigBuilder;
use crate::sync_changeset::SyncChangeset;

#[mononoke::fbinit_test]
async fn test_add_sync_target_simple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target.clone(), version.clone())
        .await?;
    add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.repo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.repo, target_cs_id).await?;

    let state =
        CommitRemappingState::read_state_from_commit(&ctx, &test.repo, target_cs_id).await?;
    assert_eq!(
        state.get_latest_synced_changeset(&first_source_name),
        Some(&first_source_cs_id),
    );
    assert_eq!(
        state.get_latest_synced_changeset(&second_source_name),
        Some(&second_source_cs_id),
    );
    assert_eq!(state.sync_config_version(), &version);

    // Remove file with commit remapping state because it's never present in source
    assert!(
        wc.remove(&NonRootMPath::new(REMAPPING_STATE_FILE)?)
            .is_some()
    );
    assert!(
        wc.remove(&NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?)
            .is_some()
    );

    assert_eq!(
        wc,
        hashmap! {
            NonRootMPath::new("source_1/first")? => "first".to_string(),
            NonRootMPath::new("source_2/second")? => "second".to_string(),
        }
    );

    // Sync a few changesets on top of target
    let cs_id = CreateCommitContext::new(&ctx, &test.repo, vec![first_source_cs_id])
        .add_file("first", "first_updated")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(cs_id)
        .await?;

    let sync_changeset =
        SyncChangeset::new(&configs_storage, &test.mononoke, &test.megarepo_mapping);

    sync_changeset
        .sync(&ctx, cs_id, &first_source_name, &target, target_cs_id)
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.repo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.repo, target_cs_id).await?;
    // Remove file with commit remapping state because it's never present in source
    assert!(
        wc.remove(&NonRootMPath::new(REMAPPING_STATE_FILE)?)
            .is_some()
    );
    assert!(
        wc.remove(&NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?)
            .is_some()
    );

    assert_eq!(
        wc,
        hashmap! {
            NonRootMPath::new("source_1/first")? => "first_updated".to_string(),
            NonRootMPath::new("source_2/second")? => "second".to_string(),
        }
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_with_linkfiles(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .linkfile("first", "linkfiles/first")
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .linkfile("second", "linkfiles/second")
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target, version.clone())
        .await?;
    add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.repo, "target").await?;
    let mut wc = list_working_copy_utf8_with_types(&ctx, &test.repo, target_cs_id).await?;

    // Remove file with commit remapping state because it's never present in source
    assert!(
        wc.remove(&NonRootMPath::new(REMAPPING_STATE_FILE)?)
            .is_some()
    );
    assert!(
        wc.remove(&NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?)
            .is_some()
    );

    assert_eq!(
        wc,
        hashmap! {
            NonRootMPath::new("source_1/first")? => ("first".to_string(), FileType::Regular),
            NonRootMPath::new("source_2/second")? => ("second".to_string(), FileType::Regular),
            NonRootMPath::new("linkfiles/first")? => ("../source_1/first".to_string(), FileType::Symlink),
            NonRootMPath::new("linkfiles/second")? => ("../source_2/second".to_string(), FileType::Symlink),
        }
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_invalid_same_prefix(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    // Use the same prefix so that files from different repos collided
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .default_prefix("prefix")
        .bookmark("source_1")
        .build_source()?
        .source_builder(second_source_name.clone())
        .default_prefix("prefix")
        .bookmark("source_2")
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("file", "content")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("file", "content")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target, version.clone())
        .await?;
    let res = add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await;

    assert!(res.is_err());

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_same_file_different_prefix(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("file", "file")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("file", "file")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target.clone(), version.clone())
        .await?;
    add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.repo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.repo, target_cs_id).await?;

    let state =
        CommitRemappingState::read_state_from_commit(&ctx, &test.repo, target_cs_id).await?;
    assert_eq!(
        state.get_latest_synced_changeset(&first_source_name),
        Some(&first_source_cs_id),
    );
    assert_eq!(
        state.get_latest_synced_changeset(&second_source_name),
        Some(&second_source_cs_id),
    );
    assert_eq!(state.sync_config_version(), &version);

    // Remove file with commit remapping state because it's never present in source
    assert!(
        wc.remove(&NonRootMPath::new(REMAPPING_STATE_FILE)?)
            .is_some()
    );
    assert!(
        wc.remove(&NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?)
            .is_some()
    );

    assert_eq!(
        wc,
        hashmap! {
            NonRootMPath::new("source_1/file")? => "file".to_string(),
            NonRootMPath::new("source_2/file")? => "file".to_string(),
        }
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_invalid_linkfiles(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .linkfile("first", "linkfiles/linkfile")
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .linkfile("second", "linkfiles/linkfile")
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target, version.clone())
        .await?;
    let res = add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await;

    assert!(res.is_err());

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_invalid_hash_to_merge(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    let second_source_cs_id = CreateCommitContext::new(&ctx, &test.repo, vec![first_source_cs_id])
        .add_file("second", "second")
        .commit()
        .await?;

    let first_source_name = SourceName::new("source_1");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .default_prefix("first_source_name")
        // Note that we set first_source_cs_id, but changeset_to_merge is has second_source_cs_id
        // this is invalid and hence add_sync_target should fail
        .source_changeset(first_source_cs_id)
        .build_source()?
        .build(&mut test.configs_storage);

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target, version.clone())
        .await?;
    let res = add_sync_target
        .run(
            &ctx,
            sync_target_config.clone(),
            btreemap! {
                first_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await;

    assert!(res.is_err());

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_merge_three_sources(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let third_source_name = SourceName::new("source_3");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .source_builder(third_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let third_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("third", "third")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, third_source_name.to_string())
        .set_to(third_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target, version.clone())
        .await?;
    add_sync_target
        .run(
            &ctx,
            sync_target_config.clone(),
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
                third_source_name.clone() => third_source_cs_id,
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.repo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.repo, target_cs_id).await?;
    // Remove file with commit remapping state because it's never present in source
    assert!(
        wc.remove(&NonRootMPath::new(REMAPPING_STATE_FILE)?)
            .is_some()
    );
    assert!(
        wc.remove(&NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?)
            .is_some()
    );

    assert_eq!(
        wc,
        hashmap! {
            NonRootMPath::new("source_1/first")? => "first".to_string(),
            NonRootMPath::new("source_2/second")? => "second".to_string(),
            NonRootMPath::new("source_3/third")? => "third".to_string(),
        }
    );

    // Validate the shape of the graph
    // It should look like
    //       o
    //      / \
    //     o   M
    //    / \

    let target_cs = target_cs_id.load(&ctx, test.repo.repo_blobstore()).await?;
    assert!(target_cs.is_merge());

    let parents = target_cs.parents().collect::<Vec<_>>();
    assert_eq!(parents.len(), 2);

    let first_merge = parents[0].load(&ctx, test.repo.repo_blobstore()).await?;
    assert!(first_merge.is_merge());

    let move_commit = parents[1].load(&ctx, test.repo.repo_blobstore()).await?;
    assert!(!move_commit.is_merge());

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_add_sync_target_repeat_same_request(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let second_source_name = SourceName::new("source_2");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.repo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.repo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let repo = add_sync_target
        .find_repo_by_id(&ctx, target.repo_id)
        .await?;
    let repo_config = repo.repo().repo_config_arc();

    let mut sync_target_config = test
        .configs_storage
        .get_config_by_version(ctx.clone(), repo_config, target.clone(), version.clone())
        .await?;
    let first_result = add_sync_target
        .run(
            &ctx,
            sync_target_config.clone(),
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    // Now repeat the same request again (as if client retries a request that has already
    // succeeded). We should get the same result as the first time.
    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let second_result = add_sync_target
        .run(
            &ctx,
            sync_target_config.clone(),
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    assert_eq!(first_result, second_result);

    // Now modify the request - it should fail
    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    assert!(
        add_sync_target
            .run(
                &ctx,
                sync_target_config.clone(),
                btreemap! {
                    first_source_name.clone() => first_source_cs_id,
                },
                None,
            )
            .await
            .is_err()
    );

    // Now send different config with the same name - should fail
    sync_target_config.sources = vec![];
    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
    let res = add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await;
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(format!("{}", err).contains("it's different from the one sent in request parameters"));

    Ok(())
}
