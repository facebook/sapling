/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use clap::Args;
use clap::Subcommand;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use commit_id::parse_commit_id;
use context::CoreContext;
use cross_repo_sync::Syncers;
use futures::try_join;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentityRef;
use slog::info;
use synced_commit_mapping::EquivalentWorkingCopyEntry;
use synced_commit_mapping::SyncedCommitMappingEntry;

use super::Repo;

/// Insert cross-repo mappings directly into DB
#[derive(Args)]
pub struct InsertArgs {
    #[clap(subcommand)]
    subcommand: InsertSubcommand,
}

#[derive(Subcommand)]
pub enum InsertSubcommand {
    /// Mark a pair of commits as rewritten
    Rewritten(RewrittenArgs),
    /// Mark a pair of commits as having an equivalent working copy
    EquivalentWorkingCopy(EquivalentWorkingCopyArgs),
    /// Mark a source commit in the large repo as not having a synced commit
    NotSyncCandidate(NotSyncCandidateArgs),
}

#[derive(Args)]
pub struct RewrittenArgs {
    /// Commit id in the source repo
    #[clap(long)]
    source_commit_id: String,

    /// Commit id in the target repo
    #[clap(long)]
    target_commit_id: String,

    /// Mapping version name to write to the DB
    #[clap(long)]
    version_name: String,
}

#[derive(Args)]
pub struct EquivalentWorkingCopyArgs {
    /// Commit id in the source repo
    #[clap(long)]
    source_commit_id: String,

    /// Commit id in the target repo
    #[clap(long)]
    target_commit_id: String,

    /// Mapping version name to write to the DB
    #[clap(long)]
    version_name: String,
}

#[derive(Args)]
pub struct NotSyncCandidateArgs {
    /// Commit id in the large repo
    #[clap(long)]
    large_commit_id: String,

    /// Optional mapping version name to write to the DB
    #[clap(long)]
    version_name: Option<String>,
}

pub async fn insert(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: Repo,
    target_repo: Repo,
    args: InsertArgs,
) -> Result<()> {
    let source_repo = Arc::new(source_repo);
    let target_repo = Arc::new(target_repo);

    let commit_syncers =
        create_commit_syncers_from_app(ctx, app, source_repo.clone(), target_repo.clone()).await?;

    match args.subcommand {
        InsertSubcommand::Rewritten(args) => {
            insert_rewritten(ctx, source_repo, target_repo, commit_syncers, args).await
        }
        InsertSubcommand::EquivalentWorkingCopy(args) => {
            insert_equivalent_working_copy(ctx, source_repo, target_repo, commit_syncers, args)
                .await
        }
        InsertSubcommand::NotSyncCandidate(args) => {
            insert_not_sync_candidate(ctx, commit_syncers, args).await
        }
    }
}

async fn insert_rewritten(
    ctx: &CoreContext,
    source_repo: Arc<Repo>,
    target_repo: Arc<Repo>,
    commit_syncers: Syncers<Arc<Repo>>,
    args: RewrittenArgs,
) -> Result<()> {
    let (source_cs_id, target_cs_id) = try_join!(
        parse_commit_id(ctx, &source_repo, &args.source_commit_id),
        parse_commit_id(ctx, &target_repo, &args.target_commit_id)
    )?;

    let commit_syncer = commit_syncers.large_to_small;

    let small_repo_id = commit_syncer.get_small_repo().repo_identity().id();
    let large_repo_id = commit_syncer.get_large_repo().repo_identity().id();

    let mapping_version = CommitSyncConfigVersion(args.version_name);
    if !commit_syncer.version_exists(&mapping_version).await? {
        return Err(anyhow!("{} version does not exist", mapping_version));
    }

    let mapping_entry = if small_repo_id == source_repo.repo_identity().id() {
        SyncedCommitMappingEntry {
            large_repo_id,
            small_repo_id,
            small_bcs_id: source_cs_id,
            large_bcs_id: target_cs_id,
            version_name: Some(mapping_version),
            source_repo: Some(commit_syncer.get_source_repo_type()),
        }
    } else {
        SyncedCommitMappingEntry {
            large_repo_id,
            small_repo_id,
            small_bcs_id: target_cs_id,
            large_bcs_id: source_cs_id,
            version_name: Some(mapping_version),
            source_repo: Some(commit_syncer.get_source_repo_type()),
        }
    };

    let res = commit_syncer.get_mapping().add(ctx, mapping_entry).await?;
    if res {
        info!(
            ctx.logger(),
            "successfully inserted rewritten mapping entry"
        );
        Ok(())
    } else {
        Err(anyhow!("failed to insert entry"))
    }
}

async fn insert_equivalent_working_copy(
    ctx: &CoreContext,
    source_repo: Arc<Repo>,
    target_repo: Arc<Repo>,
    commit_syncers: Syncers<Arc<Repo>>,
    args: EquivalentWorkingCopyArgs,
) -> Result<()> {
    let (source_cs_id, target_cs_id) = try_join!(
        parse_commit_id(ctx, &source_repo, &args.source_commit_id),
        parse_commit_id(ctx, &target_repo, &args.target_commit_id)
    )?;

    let commit_syncer = commit_syncers.large_to_small;

    let small_repo_id = commit_syncer.get_small_repo().repo_identity().id();
    let large_repo_id = commit_syncer.get_large_repo().repo_identity().id();

    let mapping_version = CommitSyncConfigVersion(args.version_name);
    if !commit_syncer.version_exists(&mapping_version).await? {
        return Err(anyhow!("{} version does not exist", mapping_version));
    }

    let mapping_entry = if small_repo_id == source_repo.repo_identity().id() {
        EquivalentWorkingCopyEntry {
            large_repo_id,
            small_repo_id,
            small_bcs_id: Some(source_cs_id),
            large_bcs_id: target_cs_id,
            version_name: Some(mapping_version),
        }
    } else {
        EquivalentWorkingCopyEntry {
            large_repo_id,
            small_repo_id,
            small_bcs_id: Some(target_cs_id),
            large_bcs_id: source_cs_id,
            version_name: Some(mapping_version),
        }
    };

    let res = commit_syncer
        .get_mapping()
        .insert_equivalent_working_copy(ctx, mapping_entry)
        .await?;
    if res {
        info!(
            ctx.logger(),
            "successfully inserted equivalent working copy"
        );
        Ok(())
    } else {
        Err(anyhow!("failed to insert entry"))
    }
}

async fn insert_not_sync_candidate(
    ctx: &CoreContext,
    commit_syncers: Syncers<Arc<Repo>>,
    args: NotSyncCandidateArgs,
) -> Result<()> {
    let commit_syncer = commit_syncers.large_to_small;

    let large_repo = commit_syncer.get_large_repo();
    let large_cs_id = parse_commit_id(ctx, large_repo, &args.large_commit_id).await?;

    let small_repo_id = commit_syncer.get_small_repo().repo_identity().id();
    let large_repo_id = commit_syncer.get_large_repo().repo_identity().id();

    let maybe_mapping_version = if let Some(version_name) = args.version_name {
        let mapping_version = CommitSyncConfigVersion(version_name);
        if !commit_syncer.version_exists(&mapping_version).await? {
            return Err(anyhow!("{} version does not exist", mapping_version));
        }
        Some(mapping_version)
    } else {
        None
    };

    let mapping_entry = EquivalentWorkingCopyEntry {
        large_repo_id,
        small_repo_id,
        small_bcs_id: None,
        large_bcs_id: large_cs_id,
        version_name: maybe_mapping_version,
    };

    let res = commit_syncer
        .get_mapping()
        .insert_equivalent_working_copy(ctx, mapping_entry)
        .await?;
    if res {
        info!(
            ctx.logger(),
            "successfully inserted not sync candidate entry"
        );
        Ok(())
    } else {
        Err(anyhow!("failed to insert entry"))
    }
}
