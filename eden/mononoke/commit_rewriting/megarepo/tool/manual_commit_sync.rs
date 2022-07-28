/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobstore::Loadable;
use context::CoreContext;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncer;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use std::collections::HashMap;
use synced_commit_mapping::SyncedCommitMapping;

/// This operation is useful immediately after a small repo is merged into a large repo.
/// See example below
///
/// ```text
///   B' <- manually synced commit from small repo (in small repo it is commit B)
///   |
///   BM <- "big merge"
///  /  \
/// ...  O <- big move commit i.e. commit that moves small repo files in correct location
///      |
///      A <- commit that was copied from small repo. It is identical between small and large repos.
///
/// Immediately after a small repo is merged into a large one we need to tell that a commit B and all of
/// its ancestors from small repo needs to be based on top of "big merge" commit in large repo rather than on top of
/// commit A.
/// The function below can be used to achieve exactly that.
/// ```
pub async fn manual_commit_sync<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    source_cs_id: ChangesetId,
    target_repo_parents: Option<Vec<ChangesetId>>,
    mapping_version: CommitSyncConfigVersion,
) -> Result<Option<ChangesetId>, Error> {
    if let Some(target_repo_parents) = target_repo_parents {
        let source_repo = commit_syncer.get_source_repo();
        let source_cs = source_cs_id.load(ctx, source_repo.blobstore()).await?;
        let source_parents: Vec<_> = source_cs.parents().collect();
        if source_parents.len() != target_repo_parents.len() {
            return Err(anyhow!(
                "wrong number of parents: source repo has {} parents, while {} target repo parents specified",
                source_parents.len(),
                target_repo_parents.len(),
            ));
        }

        let remapped_parents = source_parents
            .into_iter()
            .zip(target_repo_parents.into_iter())
            .collect::<HashMap<_, _>>();

        commit_syncer
            .unsafe_always_rewrite_sync_commit(
                ctx,
                source_cs_id,
                Some(remapped_parents),
                &mapping_version,
                CommitSyncContext::ManualCommitSync,
            )
            .await
    } else {
        commit_syncer
            .unsafe_always_rewrite_sync_commit(
                ctx,
                source_cs_id,
                None,
                &mapping_version,
                CommitSyncContext::ManualCommitSync,
            )
            .await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use cross_repo_sync_test_utils::init_small_large_repo;
    use cross_repo_sync_test_utils::xrepo_mapping_version_with_small_repo;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::MPath;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_manual_commit_sync(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        // Small and large repo look like that
        //
        // Small repo:
        //
        // O <- file3: "content3"
        // |
        // O <- file2: "content" <- "premove" bookmark, "megarepo_start" bookmark
        // |
        // O <- file: "content"
        //
        // Large repo
        // O <- file3: "content3"
        // |
        // O <- moves file -> prefix/file, file2 -> prefix/file2, "megarepo_start" bookmark
        // |
        // O <- file2: "content" <- "premove" bookmark
        // |
        // O <- file: "content"

        let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
        let small_to_large = syncers.small_to_large;
        let small_repo = small_to_large.get_source_repo();
        let large_repo = small_to_large.get_target_repo();

        // Create a commit on top of "premove" bookmark in a small repo, and then
        // manually sync it on top of big move bookmark.
        let premove_cs_id = resolve_cs_id(&ctx, &small_repo, "premove").await?;
        let commit_to_sync = CreateCommitContext::new(&ctx, &small_repo, vec![premove_cs_id])
            .add_file("some_other_file", "some_content")
            .commit()
            .await?;

        let bigmove = resolve_cs_id(&ctx, &large_repo, "megarepo_start").await?;

        let maybe_synced_commit = manual_commit_sync(
            &ctx,
            &small_to_large,
            commit_to_sync,
            Some(vec![bigmove]),
            xrepo_mapping_version_with_small_repo(),
        )
        .await?;

        let synced_commit = maybe_synced_commit.ok_or_else(|| anyhow!("commit was not synced"))?;
        let wc = list_working_copy_utf8(&ctx, &large_repo, synced_commit).await?;

        assert_eq!(
            hashmap! {
                MPath::new("prefix/file")? => "content".to_string(),
                MPath::new("prefix/file2")? => "content".to_string(),
                MPath::new("prefix/some_other_file")? => "some_content".to_string(),
            },
            wc
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_manual_commit_sync_select_parents_automatically(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        // Small and large repo look like that
        //
        // Small repo:
        //
        // O <- file3: "content3"
        // |
        // O <- file2: "content" <- "premove" bookmark, "megarepo_start" bookmark
        // |
        // O <- file: "content"
        //
        // Large repo
        // O <- file3: "content3"
        // |
        // O <- moves file -> prefix/file, file2 -> prefix/file2, "megarepo_start" bookmark
        // |
        // O <- file2: "content" <- "premove" bookmark
        // |
        // O <- file: "content"

        let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
        let large_to_small = syncers.large_to_small;
        let large_repo = large_to_small.get_source_repo();
        let small_repo = large_to_small.get_target_repo();

        // Create a commit on top of "master" bookmark in a large repo, and then
        // manually sync it on top of megarepo_start bookmark.
        let megarepo_start_cs_id = resolve_cs_id(&ctx, &large_repo, "master").await?;
        let commit_to_sync =
            CreateCommitContext::new(&ctx, &large_repo, vec![megarepo_start_cs_id])
                .add_file("prefix/some_other_file", "some_content")
                .commit()
                .await?;

        let maybe_synced_commit = manual_commit_sync(
            &ctx,
            &large_to_small,
            commit_to_sync,
            None,
            xrepo_mapping_version_with_small_repo(),
        )
        .await?;

        let synced_cs_id = maybe_synced_commit.ok_or_else(|| anyhow!("commit was not synced"))?;
        // Check that parents were correctly selected

        let small_repo_master = resolve_cs_id(&ctx, &small_repo, "master").await?;
        let synced_cs = synced_cs_id.load(&ctx, &small_repo.get_blobstore()).await?;
        assert_eq!(
            synced_cs.parents().collect::<Vec<_>>(),
            vec![small_repo_master]
        );

        // Check working copy
        let wc = list_working_copy_utf8(&ctx, &small_repo, synced_cs_id).await?;

        assert_eq!(
            hashmap! {
                MPath::new("file")? => "content".to_string(),
                MPath::new("file2")? => "content".to_string(),
                MPath::new("file3")? => "content3".to_string(),
                MPath::new("some_other_file")? => "some_content".to_string(),
            },
            wc
        );
        Ok(())
    }
}
