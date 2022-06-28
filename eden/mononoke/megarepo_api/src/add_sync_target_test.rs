/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::add_sync_target::AddSyncTarget;
use crate::megarepo_test_utils::MegarepoTest;
use crate::megarepo_test_utils::SyncTargetConfigBuilder;
use crate::sync_changeset::SyncChangeset;
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
use mononoke_types::FileType;
use mononoke_types::MPath;
use std::sync::Arc;
use tests_utils::bookmark;
use tests_utils::list_working_copy_utf8;
use tests_utils::list_working_copy_utf8_with_types;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target.clone(), version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;

    let state =
        CommitRemappingState::read_state_from_commit(&ctx, &test.blobrepo, target_cs_id).await?;
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
    assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

    assert_eq!(
        wc,
        hashmap! {
            MPath::new("source_1/first")? => "first".to_string(),
            MPath::new("source_2/second")? => "second".to_string(),
        }
    );

    // Sync a few changesets on top of target
    let cs_id = CreateCommitContext::new(&ctx, &test.blobrepo, vec![first_source_cs_id])
        .add_file("first", "first_updated")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(cs_id)
        .await?;

    let sync_changeset = SyncChangeset::new(
        &configs_storage,
        &test.mononoke,
        &test.megarepo_mapping,
        &test.mutable_renames,
    );

    sync_changeset
        .sync(&ctx, cs_id, &first_source_name, &target, target_cs_id)
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;
    // Remove file with commit remapping state because it's never present in source
    assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

    assert_eq!(
        wc,
        hashmap! {
            MPath::new("source_1/first")? => "first_updated".to_string(),
            MPath::new("source_2/second")? => "second".to_string(),
        }
    );

    Ok(())
}

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target, version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
    let mut wc = list_working_copy_utf8_with_types(&ctx, &test.blobrepo, target_cs_id).await?;

    // Remove file with commit remapping state because it's never present in source
    assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

    assert_eq!(
        wc,
        hashmap! {
            MPath::new("source_1/first")? => ("first".to_string(), FileType::Regular),
            MPath::new("source_2/second")? => ("second".to_string(), FileType::Regular),
            MPath::new("linkfiles/first")? => ("../source_1/first".to_string(), FileType::Symlink),
            MPath::new("linkfiles/second")? => ("../source_2/second".to_string(), FileType::Symlink),
        }
    );

    Ok(())
}

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("file", "content")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("file", "content")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target, version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("file", "file")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("file", "file")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target.clone(), version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;

    let state =
        CommitRemappingState::read_state_from_commit(&ctx, &test.blobrepo, target_cs_id).await?;
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
    assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

    assert_eq!(
        wc,
        hashmap! {
            MPath::new("source_1/file")? => "file".to_string(),
            MPath::new("source_2/file")? => "file".to_string(),
        }
    );

    Ok(())
}

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target, version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

#[fbinit::test]
async fn test_add_sync_target_invalid_hash_to_merge(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    println!("Create initial source commits and bookmarks");
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    let second_source_cs_id =
        CreateCommitContext::new(&ctx, &test.blobrepo, vec![first_source_cs_id])
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

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target, version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let third_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("third", "third")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, third_source_name.to_string())
        .set_to(third_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target, version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
    let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;
    // Remove file with commit remapping state because it's never present in source
    assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

    assert_eq!(
        wc,
        hashmap! {
            MPath::new("source_1/first")? => "first".to_string(),
            MPath::new("source_2/second")? => "second".to_string(),
            MPath::new("source_3/third")? => "third".to_string(),
        }
    );

    // Validate the shape of the graph
    // It should look like
    //       o
    //      / \
    //     o   M
    //    / \

    let target_cs = target_cs_id.load(&ctx, test.blobrepo.blobstore()).await?;
    assert!(target_cs.is_merge());

    let parents = target_cs.parents().collect::<Vec<_>>();
    assert_eq!(parents.len(), 2);

    let first_merge = parents[0].load(&ctx, test.blobrepo.blobstore()).await?;
    assert!(first_merge.is_merge());

    let move_commit = parents[1].load(&ctx, test.blobrepo.blobstore()).await?;
    assert!(!move_commit.is_merge());

    Ok(())
}

#[fbinit::test]
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
    let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, first_source_name.to_string())
        .set_to(first_source_cs_id)
        .await?;

    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("second", "second")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let mut sync_target_config =
        test.configs_storage
            .get_config_by_version(ctx.clone(), target.clone(), version.clone())?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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

    // Now repeat the same request again (as if client retries a reqeust that has already
    // succeeded). We should get the same result as the first time.
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
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
