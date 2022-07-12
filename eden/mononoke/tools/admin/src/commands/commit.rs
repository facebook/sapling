/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod pushrebase;
mod rebase;
mod split;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use changeset_fetcher::ChangesetFetcher;
use changeset_fetcher::ChangesetFetcherArc;
use changeset_fetcher::ChangesetFetcherRef;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::TryStreamExt;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

use self::pushrebase::CommitPushrebaseArgs;
use self::rebase::CommitRebaseArgs;
use self::split::CommitSplitArgs;

/// Manipulate commits
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(subcommand)]
    subcommand: CommitSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    changesets: dyn Changesets,

    #[facet]
    changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,
}

#[derive(Subcommand)]
pub enum CommitSubcommand {
    /// Split a large commit into a stack
    ///
    /// Attempts to maintain limits on the number of files and size of all the
    /// files in each of the commits, however these limits are not strictly
    /// enforced, i.e. some of the commits might have larger sizes or more
    /// files, e.g. if a single file is larger than the limit, or if there are
    /// a large number of grouped copy sources and their destinations.
    ///
    /// The stack is printed in order from ancestor to descendant.
    Split(CommitSplitArgs),

    /// Rebase a commit
    ///
    /// Rebases a commit from its current parent to a new parent.  This is a
    /// low-level command and does not perform any validation on the rebase.
    /// The caller must ensure that the result of this rebase is valid.
    Rebase(CommitRebaseArgs),

    /// Pushrebase a commit
    ///
    /// Rebases a commit from its current bookmark onto a bookmark, and moves
    /// that bookmark to the newly rebased commit.
    Pushrebase(CommitPushrebaseArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        CommitSubcommand::Split(split_args) => split::split(&ctx, &repo, split_args).await?,
        CommitSubcommand::Rebase(rebase_args) => rebase::rebase(&ctx, &repo, rebase_args).await?,
        CommitSubcommand::Pushrebase(pushrebase_args) => {
            pushrebase::pushrebase(&ctx, &repo, pushrebase_args).await?
        }
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
