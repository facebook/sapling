/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod ancestors_difference;
mod backfill;
mod backfill_one;
mod checkpoints;
mod children;
mod common_base;
mod range_stream;
mod segments;
mod slice_ancestors;
mod update_preloaded;

use ancestors_difference::AncestorsDifferenceArgs;
use anyhow::Result;
use backfill::BackfillArgs;
use backfill_one::BackfillOneArgs;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use children::ChildrenArgs;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use common_base::CommonBaseArgs;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use range_stream::RangeStreamArgs;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;
use segments::SegmentsArgs;
use slice_ancestors::SliceAncestorsArgs;
use update_preloaded::UpdatePreloadedArgs;

/// Query and manage the commit graph
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: CommitGraphSubcommand,
}

#[derive(Subcommand)]
pub enum CommitGraphSubcommand {
    /// Backfill commit graph entries
    Backfill(BackfillArgs),
    /// Backfill a commit and all of its missing ancestors.
    BackfillOne(BackfillOneArgs),
    /// Display ids of all commits that are ancestors of one set of commits (heads),
    /// excluding ancestors of another set of commits (common) in reverse topological order.
    AncestorsDifference(AncestorsDifferenceArgs),
    /// Display ids of all commits that are simultaneously a descendant of one commit (start)
    /// and an ancestor of another (end) in topological order.
    RangeStream(RangeStreamArgs),
    /// Update preloaded commit graph and upload it to blobstore.
    UpdatePreloaded(UpdatePreloadedArgs),
    /// Display ids of all the highest generation commits among the common ancestors of two commits.
    CommonBase(CommonBaseArgs),
    /// Slices ancestors of given commits and displays commits IDs of frontiers for each slice.
    SliceAncestors(SliceAncestorsArgs),
    /// Display ids of all children commits of a given commit.
    Children(ChildrenArgs),
    /// Display segments representing ancestors of one set of commits (heads), excluding
    /// ancestors of another set of commits (common) in reverse topological order.
    Segments(SegmentsArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    changesets: dyn Changesets,

    #[facet]
    changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    config: RepoConfig,

    #[facet]
    id: RepoIdentity,

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
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        CommitGraphSubcommand::Backfill(args) => backfill::backfill(&ctx, &app, &repo, args).await,
        CommitGraphSubcommand::BackfillOne(args) => {
            backfill_one::backfill_one(&ctx, &repo, args).await
        }
        CommitGraphSubcommand::AncestorsDifference(args) => {
            ancestors_difference::ancestors_difference(&ctx, &repo, args).await
        }
        CommitGraphSubcommand::RangeStream(args) => {
            range_stream::range_stream(&ctx, &repo, args).await
        }
        CommitGraphSubcommand::UpdatePreloaded(args) => {
            update_preloaded::update_preloaded(&ctx, &app, &repo, args).await
        }
        CommitGraphSubcommand::CommonBase(args) => {
            common_base::common_base(&ctx, &repo, args).await
        }
        CommitGraphSubcommand::SliceAncestors(args) => {
            slice_ancestors::slice_ancestors(&ctx, &repo, args).await
        }
        CommitGraphSubcommand::Children(args) => children::children(&ctx, &repo, args).await,
        CommitGraphSubcommand::Segments(args) => segments::segments(&ctx, &repo, args).await,
    }
}
