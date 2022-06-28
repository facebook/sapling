/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::add_branching_sync_target::AddBranchingSyncTarget;
use crate::add_sync_target::AddSyncTarget;
use crate::megarepo_test_utils::MegarepoTest;
use crate::megarepo_test_utils::SyncTargetConfigBuilder;
use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::btreemap;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Target;
use megarepo_mapping::SourceName;
use std::sync::Arc;
use tests_utils::bookmark;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;

#[fbinit::test]
async fn test_add_branching_sync_target_success(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
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
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;

    println!("Create new release branch");
    let new_target = test.target("release".to_string());
    let add_branching_sync_target = AddBranchingSyncTarget::new(&configs_storage, &test.mononoke);
    let new_config = add_branching_sync_target
        .fork_new_sync_target_config(&ctx, new_target, target_cs_id, target.clone())
        .await?;
    let new_cs_id = add_branching_sync_target
        .run(&ctx, new_config, target_cs_id)
        .await?;
    assert_eq!(
        new_cs_id,
        resolve_cs_id(&ctx, &test.blobrepo, "release").await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_add_branching_sync_target_no_source(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());
    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let target_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("first", "first")
        .commit()
        .await?;

    let new_target = test.target("release".to_string());
    let add_branching_sync_target = AddBranchingSyncTarget::new(&configs_storage, &test.mononoke);
    assert!(
        add_branching_sync_target
            .fork_new_sync_target_config(&ctx, new_target, target_cs_id, target.clone())
            .await
            .is_err(),
        "Found a sync target config that doesn't exist!"
    );

    Ok(())
}

#[fbinit::test]
async fn test_add_branching_sync_target_wrong_branch(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut test = MegarepoTest::new(&ctx).await?;
    let target: Target = test.target("target".to_string());

    let first_source_name = SourceName::new("source_1");
    let version = "version_1".to_string();
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
        .source_builder(first_source_name.clone())
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
            },
            None,
        )
        .await?;

    let second_source_name = SourceName::new("source_2");
    let alt_version = "alt_version_1".to_string();
    let alt_target: Target = test.target("alt_target".to_string());
    SyncTargetConfigBuilder::new(test.repo_id(), alt_target.clone(), alt_version.clone())
        .source_builder(second_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    println!("Create alternate source commits and bookmarks");
    let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("third", "third")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, second_source_name.to_string())
        .set_to(second_source_cs_id)
        .await?;

    let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage.clone());

    let sync_target_config = test.configs_storage.get_config_by_version(
        ctx.clone(),
        alt_target.clone(),
        alt_version.clone(),
    )?;
    let add_sync_target =
        AddSyncTarget::new(&configs_storage, &test.mononoke, &test.mutable_renames);
    add_sync_target
        .run(
            &ctx,
            sync_target_config,
            btreemap! {
                second_source_name.clone() => second_source_cs_id,
            },
            None,
        )
        .await?;

    let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "alt_target").await?;

    println!("Create new release branch against alt_target's unification commit");
    let new_target = test.target("release".to_string());
    let add_branching_sync_target = AddBranchingSyncTarget::new(&configs_storage, &test.mononoke);
    assert!(
        add_branching_sync_target
            .fork_new_sync_target_config(&ctx, new_target, target_cs_id, target.clone())
            .await
            .is_err(),
        "Found a sync target config for the wrong branch"
    );

    Ok(())
}
