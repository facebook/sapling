/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use commit_id::parse_commit_id;
use context::CoreContext;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::Repo as CrossRepo;
use cross_repo_sync::unsafe_always_rewrite_sync_commit;
use futures::future::try_join_all;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_types::ChangesetId;
use slog::info;

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
pub async fn manual_commit_sync<R: CrossRepo>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    source_cs_id: ChangesetId,
    target_repo_parents: Option<Vec<ChangesetId>>,
    mapping_version: CommitSyncConfigVersion,
) -> Result<Option<ChangesetId>, Error> {
    if let Some(target_repo_parents) = target_repo_parents {
        let source_repo = commit_sync_data.get_source_repo();
        let source_cs = source_cs_id.load(ctx, source_repo.repo_blobstore()).await?;
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

        unsafe_always_rewrite_sync_commit(
            ctx,
            source_cs_id,
            commit_sync_data,
            Some(remapped_parents),
            &mapping_version,
            CommitSyncContext::ManualCommitSync,
        )
        .await
    } else {
        unsafe_always_rewrite_sync_commit(
            ctx,
            source_cs_id,
            commit_sync_data,
            None,
            &mapping_version,
            CommitSyncContext::ManualCommitSync,
        )
        .await
    }
}

/// Manually sync a commit from source repo to a target repo. It's usually used right after a big merge
#[derive(Debug, clap::Args)]
pub struct ManualCommitSyncArgs {
    #[clap(flatten)]
    repo_args: SourceAndTargetRepoArgs,

    /// Source repo changeset that will synced to target repo
    #[clap(long)]
    commit: String,

    /// Parents of the new commit
    #[clap(long, conflicts_with = "select_parents_automatically")]
    parents: Vec<String>,

    /// Finds parents automatically: takes parents in the source repo and finds equivalent commits in target repo.
    /// If parents are not remapped yet then this command will fail
    #[clap(long, conflicts_with = "parents")]
    select_parents_automatically: bool,

    /// Dry-run mode - doesn't do a merge, just validates
    #[clap(long)]
    dry_run: bool,

    /// Name of the noop mapping that will be inserted
    #[clap(long)]
    mapping_version_name: String,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: ManualCommitSyncArgs) -> Result<()> {
    let source_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        source_repo.repo_identity().name(),
        source_repo.repo_identity().id()
    );

    let target_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;
    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        target_repo.repo_identity().name(),
        target_repo.repo_identity().id()
    );

    let commit_sync_data =
        create_single_direction_commit_syncer(ctx, &app, source_repo.clone(), target_repo.clone())
            .await?;
    let target_repo_parents = if args.select_parents_automatically {
        None
    } else {
        Some(
            try_join_all(args.parents.iter().map(async |p| {
                let id: ChangesetId = parse_commit_id(ctx, &target_repo, p).await?;
                info!(ctx.logger(), "changeset resolved as: {:?}", id);
                Result::<_>::Ok(id)
            }))
            .await?,
        )
    };
    let source_cs = parse_commit_id(ctx, &source_repo, &args.commit).await?;
    info!(ctx.logger(), "changeset resolved as: {:?}", source_cs);

    let target_cs_id = manual_commit_sync(
        ctx,
        &commit_sync_data,
        source_cs,
        target_repo_parents,
        CommitSyncConfigVersion(args.mapping_version_name),
    )
    .await?;
    info!(ctx.logger(), "target cs id is {:?}", target_cs_id);
    Ok(())
}

#[cfg(test)]
mod test {
    use cross_repo_sync::test_utils::TestRepo;
    use cross_repo_sync::test_utils::init_small_large_repo;
    use cross_repo_sync::test_utils::xrepo_mapping_version_with_small_repo;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use tests_utils::CreateCommitContext;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::resolve_cs_id;

    use super::*;

    #[mononoke::fbinit_test]
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

        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
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
                NonRootMPath::new("prefix/file")? => "content".to_string(),
                NonRootMPath::new("prefix/file2")? => "content".to_string(),
                NonRootMPath::new("prefix/some_other_file")? => "some_content".to_string(),
            },
            wc
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
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

        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
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
        let synced_cs = synced_cs_id
            .load(&ctx, &small_repo.repo_blobstore().clone())
            .await?;
        assert_eq!(
            synced_cs.parents().collect::<Vec<_>>(),
            vec![small_repo_master]
        );

        // Check working copy
        let wc = list_working_copy_utf8(&ctx, &small_repo, synced_cs_id).await?;

        assert_eq!(
            hashmap! {
                NonRootMPath::new("file")? => "content".to_string(),
                NonRootMPath::new("file2")? => "content".to_string(),
                NonRootMPath::new("file3")? => "content3".to_string(),
                NonRootMPath::new("some_other_file")? => "some_content".to_string(),
            },
            wc
        );
        Ok(())
    }
}
