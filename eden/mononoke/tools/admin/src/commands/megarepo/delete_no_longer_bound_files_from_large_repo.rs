/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use context::CoreContext;
use cross_repo_sync::CommitSyncData;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use megarepolib::common::StackPosition;
use megarepolib::common::create_and_save_bonsai;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use movers::Mover;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use slog::info;

use super::common::LightResultingChangesetArgs;
use super::common::get_commit_factory;

/// Right after small and large are bound usually a majority of small repo
/// files map to a single folder in large repo (let's call it DIR).
/// Later these files from small repo might be bound to a another files in
/// large repo however files in DIR might still exist in large repo.
///
/// This command allows us to delete these files from DIR. It does so by
/// finding all files in DIR and its subfolders that do not remap to a
/// small repo and then deleting them.
///
/// Note: if there are files in DIR that were never part of a bind,
/// they will be deleted.
#[derive(Debug, clap::Args)]
pub struct DeleteNoLongerBoundFilesFromLargeRepoArgs {
    #[clap(flatten)]
    pub repo_args: SourceAndTargetRepoArgs,

    #[command(flatten)]
    pub res_cs_args: LightResultingChangesetArgs,

    #[clap(flatten)]
    pub commit: ChangesetArgs,

    /// Path prefix where to search for files to delete from
    #[clap(long)]
    pub path_prefix: String,
}

pub async fn run(
    ctx: &CoreContext,
    app: MononokeApp,
    args: DeleteNoLongerBoundFilesFromLargeRepoArgs,
) -> Result<()> {
    let source_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    let target_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;

    let syncers = create_commit_syncers_from_app(ctx, &app, source_repo, target_repo).await?;
    let commit_syncer = syncers.large_to_small;
    let large_repo = commit_syncer.get_large_repo();

    let cs_id = args
        .commit
        .resolve_changeset(ctx, commit_syncer.get_source_repo())
        .await?;

    // Find all files under a given path
    let root_fsnode_id = large_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;
    let entries = root_fsnode_id
        .fsnode_id()
        .find_entries(
            ctx.clone(),
            large_repo.repo_blobstore().clone(),
            vec![PathOrPrefix::Prefix(MPath::new(args.path_prefix)?)],
        )
        .try_collect::<Vec<_>>()
        .await?;

    // Now find which files does not remap to a small repo - these files we want to delete
    let mover = find_mover_for_commit(ctx, &commit_syncer, cs_id).await?;

    let mut to_delete = vec![];
    for (path, entry) in entries {
        if let Entry::Leaf(_) = entry {
            let path = path.try_into().unwrap();
            if mover.move_path(&path)?.is_none() {
                to_delete.push(path);
            }
        }
    }

    if to_delete.is_empty() {
        info!(ctx.logger(), "nothing to delete, exiting");
        return Ok(());
    }
    info!(ctx.logger(), "need to delete {} paths", to_delete.len());

    let cs_args_factory = get_commit_factory(args.res_cs_args, |s, _num| s.to_string())?;
    let cs_args = cs_args_factory(StackPosition(0));
    let deletion_cs_id = create_and_save_bonsai(
        ctx,
        large_repo,
        vec![cs_id],
        to_delete
            .into_iter()
            .map(|file| (file, FileChange::Deletion))
            .collect(),
        cs_args,
    )
    .await?;

    info!(ctx.logger(), "created changeset {}", deletion_cs_id);

    Ok(())
}

async fn find_mover_for_commit<R: cross_repo_sync::Repo>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncData<R>,
    cs_id: ChangesetId,
) -> Result<Arc<dyn Mover>, Error> {
    let maybe_sync_outcome = commit_syncer.get_commit_sync_outcome(ctx, cs_id).await?;

    let sync_outcome = maybe_sync_outcome.context("source commit was not remapped yet")?;
    use cross_repo_sync::CommitSyncOutcome::*;
    let mover = match sync_outcome {
        NotSyncCandidate(_) => {
            return Err(format_err!(
                "commit is a not sync candidate, can't get a mover for this commit"
            ));
        }
        RewrittenAs(_, version) | EquivalentWorkingCopyAncestor(_, version) => {
            commit_syncer.get_movers_by_version(&version).await?.mover
        }
    };

    Ok(mover)
}
