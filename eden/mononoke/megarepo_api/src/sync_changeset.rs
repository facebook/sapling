/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{find_bookmark_and_value, MegarepoOp};
use anyhow::anyhow;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use commit_transformation::{create_source_to_target_multi_mover, rewrite_commit, upload_commits};
use context::CoreContext;
use megarepo_config::{
    MononokeMegarepoConfigs, Source, SourceMappingRules, SourceRevision, SyncTargetConfig, Target,
};
use megarepo_error::MegarepoError;
use megarepo_mapping::{CommitRemappingState, MegarepoMapping};
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct SyncChangeset<'a> {
    megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    mononoke: &'a Arc<Mononoke>,
    target_megarepo_mapping: &'a Arc<MegarepoMapping>,
}

#[async_trait]
impl<'a> MegarepoOp for SyncChangeset<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        &self.mononoke
    }
}

impl<'a> SyncChangeset<'a> {
    pub(crate) fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke>,
        target_megarepo_mapping: &'a Arc<MegarepoMapping>,
    ) -> Self {
        Self {
            megarepo_configs,
            mononoke,
            target_megarepo_mapping,
        }
    }

    pub(crate) async fn sync(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        source_name: &String,
        target: &Target,
    ) -> Result<ChangesetId, MegarepoError> {
        let target_repo = self.find_repo_by_id(&ctx, target.repo_id).await?;

        // Now we need to find the target config version that was used to create the latest
        // target commit. This config version will be used to sync the new changeset
        let (target_bookmark, target_cs_id) =
            find_target_bookmark_and_value(&ctx, &target_repo, &target).await?;

        let (commit_remapping_state, target_config) = find_target_sync_config(
            &ctx,
            target_repo.blob_repo(),
            target_cs_id,
            &target,
            &self.megarepo_configs,
        )
        .await?;

        // Given the SyncTargetConfig, let's find config for the source
        // we are going to sync from
        let source_config = find_source_config(&source_name, &target_config)?;

        // Find source repo and changeset that we need to sync
        let source_repo = self.find_repo_by_id(&ctx, source_config.repo_id).await?;
        let source_cs = source_cs_id
            .load(&ctx, source_repo.blob_repo().blobstore())
            .await?;

        // Check if we can sync this commit at all
        if source_cs.is_merge() {
            return Err(MegarepoError::request(anyhow!(
                "{} is a merge commit, and syncing of merge commits is not supported yet",
                source_cs.get_changeset_id()
            )));
        }
        validate_can_sync_changeset(
            &ctx,
            &target,
            &source_cs,
            &commit_remapping_state,
            &source_repo,
            &source_config,
        )
        .await?;

        // Finally create a commit in the target and update the mapping.
        let source_cs_id = source_cs.get_changeset_id();
        let new_target_cs_id = sync_changeset_to_target(
            &ctx,
            &source_config.mapping,
            &source_name,
            source_repo.blob_repo(),
            source_cs,
            target_repo.blob_repo(),
            target_cs_id,
            &target,
            commit_remapping_state,
        )
        .await?;

        self.target_megarepo_mapping
            .insert_source_target_cs_mapping(
                &ctx,
                &source_name,
                &target,
                source_cs_id,
                new_target_cs_id,
                &target_config.version,
            )
            .await?;

        // Move the bookmark and record latest synced source changeset
        let res = update_target_bookmark(
            &ctx,
            target_repo.blob_repo(),
            target_bookmark,
            target_cs_id,
            new_target_cs_id,
        )
        .await?;

        if !res {
            // TODO(stash): we might want a special exception type for this case
            return Err(MegarepoError::request(anyhow!(
                "race condition - target bookmark moved while request was executing",
            )));
        }

        Ok(new_target_cs_id)
    }
}

async fn find_target_bookmark_and_value(
    ctx: &CoreContext,
    target_repo: &RepoContext,
    target: &Target,
) -> Result<(BookmarkName, ChangesetId), MegarepoError> {
    find_bookmark_and_value(ctx, target_repo, &target.bookmark).await
}

async fn find_target_sync_config<'a>(
    ctx: &'a CoreContext,
    target_repo: &'a BlobRepo,
    target_cs_id: ChangesetId,
    target: &Target,
    megarepo_configs: &Arc<dyn MononokeMegarepoConfigs>,
) -> Result<(CommitRemappingState, SyncTargetConfig), MegarepoError> {
    let state =
        CommitRemappingState::read_state_from_commit(ctx, target_repo, target_cs_id).await?;

    // We have a target config version - let's fetch target config itself.
    let target_config = megarepo_configs.get_config_by_version(
        ctx.clone(),
        target.clone(),
        state.sync_config_version().clone(),
    )?;

    Ok((state, target_config))
}

fn find_source_config<'a, 'b>(
    source_name: &'a str,
    target_config: &'b SyncTargetConfig,
) -> Result<&'b Source, MegarepoError> {
    let mut maybe_source_config = None;
    for source in &target_config.sources {
        if source_name == source.source_name {
            maybe_source_config = Some(source);
            break;
        }
    }
    let source_config = maybe_source_config.ok_or_else(|| {
        MegarepoError::request(anyhow!("config for source {} not found", source_name))
    })?;

    Ok(source_config)
}

// We allow syncing changeset from a source if one of its parents was the latest synced changeset
// from this source into this target.
async fn validate_can_sync_changeset(
    ctx: &CoreContext,
    target: &Target,
    source_cs: &BonsaiChangeset,
    commit_remapping_state: &CommitRemappingState,
    source_repo: &RepoContext,
    source: &Source,
) -> Result<(), MegarepoError> {
    match &source.revision {
        SourceRevision::hash(_) => {
            return Err(MegarepoError::request(anyhow!(
                "can't sync changeset from source {} because this source points to a changeset",
                source.source_name,
            )));
        }
        SourceRevision::bookmark(bookmark) => {
            let (_, source_bookmark_value) =
                find_bookmark_and_value(ctx, source_repo, &bookmark).await?;

            if source_bookmark_value != source_cs.get_changeset_id() {
                let is_ancestor = source_repo
                    .skiplist_index()
                    .is_ancestor(
                        ctx,
                        &source_repo.blob_repo().get_changeset_fetcher(),
                        source_cs.get_changeset_id(),
                        source_bookmark_value,
                    )
                    .await
                    .map_err(MegarepoError::internal)?;

                if !is_ancestor {
                    return Err(MegarepoError::request(anyhow!(
                        "{} is not an ancestor of source bookmark {}",
                        source_bookmark_value,
                        bookmark,
                    )));
                }
            }
        }
        SourceRevision::UnknownField(_) => {
            return Err(MegarepoError::internal(anyhow!(
                "unexpected source revision!"
            )));
        }
    };

    let maybe_latest_synced_cs_id =
        commit_remapping_state.get_latest_synced_changeset(&source.source_name);

    match maybe_latest_synced_cs_id {
        Some(latest_synced_cs_id) => {
            let found = source_cs.parents().find(|p| p == latest_synced_cs_id);
            if found.is_none() {
                return Err(MegarepoError::request(anyhow!(
                    "Can't sync {}, because latest synced commit is not a parent of this commit. \
                            Latest synced source changeset is {}",
                    source_cs.get_changeset_id(),
                    latest_synced_cs_id,
                )));
            }
        }
        None => {
            return Err(MegarepoError::internal(anyhow!(
                "Source {:?} was not synced into target {:?}",
                source.source_name,
                target
            )));
        }
    };

    Ok(())
}

async fn sync_changeset_to_target(
    ctx: &CoreContext,
    mapping: &SourceMappingRules,
    source: &str,
    source_repo: &BlobRepo,
    source_cs: BonsaiChangeset,
    target_repo: &BlobRepo,
    target_cs_id: ChangesetId,
    target: &Target,
    mut state: CommitRemappingState,
) -> Result<ChangesetId, MegarepoError> {
    let mover =
        create_source_to_target_multi_mover(mapping.clone()).map_err(MegarepoError::internal)?;

    let source_cs_id = source_cs.get_changeset_id();
    // Create a new commit using a mover
    let source_cs_mut = source_cs.into_mut();
    let mut remapped_parents = HashMap::new();
    match (source_cs_mut.parents.get(0), source_cs_mut.parents.get(1)) {
        (Some(parent), None) => {
            remapped_parents.insert(*parent, target_cs_id);
        }
        _ => {
            return Err(MegarepoError::request(anyhow!(
                "expected exactly one parent, found {}",
                source_cs_mut.parents.len()
            )));
        }
    }

    let mut rewritten_commit = rewrite_commit(
        &ctx,
        source_cs_mut,
        &remapped_parents,
        mover,
        source_repo.clone(),
        None,
    )
    .await
    .map_err(MegarepoError::internal)?
    .ok_or_else(|| {
        MegarepoError::internal(anyhow!(
            "failed to rewrite commit {}, target: {:?}",
            source_cs_id,
            target
        ))
    })?;

    state.set_source_changeset(source, source_cs_id);
    state
        .save_in_changeset(ctx, target_repo, &mut rewritten_commit)
        .await?;

    let rewritten_commit = rewritten_commit.freeze().map_err(MegarepoError::internal)?;
    let target_cs_id = rewritten_commit.get_changeset_id();
    upload_commits(&ctx, vec![rewritten_commit], source_repo, target_repo)
        .await
        .map_err(MegarepoError::internal)?;

    Ok(target_cs_id)
}

async fn update_target_bookmark(
    ctx: &CoreContext,
    target_repo: &BlobRepo,
    bookmark: BookmarkName,
    from_target_cs_id: ChangesetId,
    to_target_cs_id: ChangesetId,
) -> Result<bool, MegarepoError> {
    let mut bookmark_txn = target_repo.bookmarks().create_transaction(ctx.clone());

    bookmark_txn
        .update(
            &bookmark,
            to_target_cs_id,
            from_target_cs_id,
            BookmarkUpdateReason::XRepoSync,
            None,
        )
        .map_err(MegarepoError::internal)?;

    let res = bookmark_txn
        .commit()
        .await
        .map_err(MegarepoError::internal)?;

    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::megarepo_test_utils::{MegarepoTest, SyncTargetConfigBuilder};
    use anyhow::Error;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use megarepo_mapping::REMAPPING_STATE_FILE;
    use mononoke_types::MPath;
    use tests_utils::{bookmark, list_working_copy_utf8, resolve_cs_id, CreateCommitContext};

    #[fbinit::test]
    async fn test_sync_changeset_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source_name = "source_1".to_string();
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commit and bookmark");
        let init_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.clone())
            .set_to(init_source_cs_id)
            .await?;

        test.prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset =
            SyncChangeset::new(&configs_storage, &test.mononoke, &test.megarepo_mapping);
        println!("Trying to sync already synced commit again");
        let res = sync_changeset
            .sync(&ctx, init_source_cs_id, &source_name, &target)
            .await;
        assert!(res.is_err());

        let source_cs_id = CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
            .add_file("anotherfile", "anothercontent")
            .commit()
            .await?;

        println!("Syncing a commit that's not ancestor of target bookmark");
        let res = sync_changeset
            .sync(&ctx, source_cs_id, &source_name, &target)
            .await;
        assert!(res.is_err());

        bookmark(&ctx, &test.blobrepo, source_name.clone())
            .set_to(source_cs_id)
            .await?;

        println!("Syncing new commit");
        sync_changeset
            .sync(&ctx, source_cs_id, &source_name, &target)
            .await?;

        let cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, cs_id).await?;

        // Remove file with commit remapping state because it's never present in source
        wc.remove(&MPath::new(REMAPPING_STATE_FILE)?);

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/file")? => "content".to_string(),
                MPath::new("source_1/anotherfile")? => "anothercontent".to_string(),
            }
        );

        Ok(())
    }
}
