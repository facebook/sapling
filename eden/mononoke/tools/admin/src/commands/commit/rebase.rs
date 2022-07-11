/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use changeset_fetcher::ChangesetFetcherArc;
use changeset_fetcher::ChangesetFetcherRef;
use clap::Args;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use repo_blobstore::RepoBlobstoreRef;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct CommitRebaseArgs {
    /// Destination Commit ID to rebase onto
    #[clap(long, short = 'd')]
    dest: String,

    /// Source Commit ID to rebase (bottom of the stack if rebasing a stack)
    #[clap(long, short = 's')]
    source: String,

    /// Top Commit ID of the source stack, if rebasing a stack
    #[clap(long, short = 't')]
    top: Option<String>,

    /// Skip rebase validity checks (only use if you know what you're doing).
    #[clap(long)]
    skip_rebase_validity_checks: bool,
}

pub async fn rebase(ctx: &CoreContext, repo: &Repo, rebase_args: CommitRebaseArgs) -> Result<()> {
    if !rebase_args.skip_rebase_validity_checks {
        bail!("You must provide --skip-rebase-validity-checks to this command");
    }

    let dest = parse_commit_id(ctx, repo, &rebase_args.dest).await?;
    let source = parse_commit_id(ctx, repo, &rebase_args.source).await?;

    if let Some(top) = &rebase_args.top {
        let top = parse_commit_id(ctx, repo, top).await?;
        let csids = resolve_stack(ctx, repo, source, top).await?;
        let mut dest = dest;
        for csid in csids {
            let rebased = rebase_single_changeset(ctx, repo, csid, dest).await?;
            println!("{}", rebased);
            dest = rebased;
        }
    } else {
        let rebased = rebase_single_changeset(ctx, repo, source, dest).await?;
        println!("{}", rebased);
    }

    Ok(())
}

async fn resolve_stack(
    ctx: &CoreContext,
    repo: &Repo,
    bottom: ChangesetId,
    top: ChangesetId,
) -> Result<Vec<ChangesetId>> {
    let mut csids =
        revset::RangeNodeStream::new(ctx.clone(), repo.changeset_fetcher_arc(), bottom, top)
            .compat()
            .map_ok(|csid| async move {
                let parents = repo
                    .changeset_fetcher()
                    .get_parents(ctx.clone(), csid)
                    .await?;
                if parents.len() > 1 {
                    bail!("rebasing stacks with merges is not supported");
                }
                Ok(csid)
            })
            .try_buffered(100)
            .try_collect::<Vec<_>>()
            .await?;
    // Reverse so that the stack is provided in topological order from bottom
    // to top.
    csids.reverse();
    Ok(csids)
}

async fn rebase_single_changeset(
    ctx: &CoreContext,
    repo: &Repo,
    source: ChangesetId,
    dest: ChangesetId,
) -> Result<ChangesetId> {
    let bcs = source
        .load(ctx, repo.repo_blobstore())
        .await
        .map_err(Error::from)?;
    let mut rebased = bcs.into_mut();
    if rebased.parents.is_empty() {
        rebased.parents.push(dest);
    } else {
        rebased.parents[0] = dest;
    }

    for file_change in rebased.file_changes.values_mut() {
        match file_change {
            FileChange::Change(fc) => {
                if let Some((_, ref mut parent)) = fc.copy_from_mut() {
                    *parent = dest;
                }
            }
            FileChange::Deletion
            | FileChange::UntrackedDeletion
            | FileChange::UntrackedChange(_) => {}
        }
    }

    let rebased = rebased.freeze()?;
    let rebased_cs_id = rebased.get_changeset_id();
    changesets_creation::save_changesets(ctx, repo, vec![rebased]).await?;
    Ok(rebased_cs_id)
}
