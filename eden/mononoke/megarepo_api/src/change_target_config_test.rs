/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::add_sync_target::AddSyncTarget;
use crate::change_target_config::ChangeTargetConfig;
use crate::megarepo_test_utils::{MegarepoTest, SyncTargetConfigBuilder};
use anyhow::Error;
use blobstore::Loadable;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::{btreemap, hashmap};
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Target;
use megarepo_mapping::{SourceName, REMAPPING_STATE_FILE};
use mononoke_types::{FileType, MPath};
use std::sync::Arc;
use tests_utils::{
    bookmark, list_working_copy_utf8_with_types, resolve_cs_id, CreateCommitContext,
};

#[fbinit::test]
async fn test_change_target_config(fb: FacebookInit) -> Result<(), Error> {
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
            .get_config_by_version(ctx.clone(), target.clone(), version.clone())?;
    let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
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

    let version_2 = "version_2".to_string();
    let third_source_name = SourceName::new("source_3");
    let third_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
        .add_file("third", "third")
        .commit()
        .await?;

    bookmark(&ctx, &test.blobrepo, third_source_name.to_string())
        .set_to(third_source_cs_id)
        .await?;
    SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version_2.clone())
        .source_builder(first_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .linkfile("first", "linkfiles/first_in_other_location")
        .build_source()?
        .source_builder(third_source_name.clone())
        .set_prefix_bookmark_to_source_name()
        .build_source()?
        .build(&mut test.configs_storage);

    let change_target_config = ChangeTargetConfig::new(&configs_storage, &test.mononoke);
    change_target_config
        .run(
            &ctx,
            &target,
            version_2,
            target_cs_id,
            btreemap! {
                first_source_name.clone() => first_source_cs_id,
                third_source_name.clone() => third_source_cs_id,
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
            MPath::new("linkfiles/first_in_other_location")? => ("../source_1/first".to_string(), FileType::Symlink),
            MPath::new("source_1/first")? => ("first".to_string(), FileType::Regular),
            MPath::new("source_3/third")? => ("third".to_string(), FileType::Regular),
        }
    );
    let target_bonsai = target_cs_id.load(&ctx, &test.blobrepo.blobstore()).await?;
    assert_eq!(
        target_bonsai
            .file_changes()
            .map(|(a, _b)| a.clone())
            .collect::<Vec<_>>(),
        vec![
            MPath::new(".megarepo/remapping_state")?,
            MPath::new("linkfiles/first")?,
            MPath::new("linkfiles/second")?,
            MPath::new("source_2/second")?
        ],
    );

    Ok(())
}
