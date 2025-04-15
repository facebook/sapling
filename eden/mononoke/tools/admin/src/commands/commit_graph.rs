/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod ancestors_difference;
mod children;
mod common_base;
mod descendants;
mod is_ancestor;
mod range_stream;
mod segments;
mod slice_ancestors;
mod update_preloaded;

use std::sync::Arc;

use ancestors_difference::AncestorsDifferenceArgs;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use children::ChildrenArgs;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use common_base::CommonBaseArgs;
use descendants::DescendantsArgs;
use is_ancestor::IsAncestorArgs;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::OptRepoArgs;
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
    repo: OptRepoArgs,

    /// Perform the commit graph operation on all applicable repos
    #[clap(long, conflicts_with_all = &["repo-name", "repo-id"])]
    all_repos: bool,

    #[clap(subcommand)]
    subcommand: CommitGraphSubcommand,
}

#[derive(Subcommand)]
pub enum CommitGraphSubcommand {
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
    /// Display ids of the union of descendants of the given commits.
    Descendants(DescendantsArgs),
    /// Display segments representing ancestors of one set of commits (heads), excluding
    /// ancestors of another set of commits (common) in reverse topological order.
    Segments(SegmentsArgs),
    /// Check if a commit is an ancestor of another commit.
    IsAncestor(IsAncestorArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

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
    let app = Arc::new(app);
    let ctx = app.new_basic_context();
    let maybe_repo: Option<Repo> = app.maybe_open_repo(args.repo.as_repo_arg()).await?;

    match (args.subcommand, maybe_repo) {
        (CommitGraphSubcommand::AncestorsDifference(args), Some(repo)) => {
            ancestors_difference::ancestors_difference(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::RangeStream(args), Some(repo)) => {
            range_stream::range_stream(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::UpdatePreloaded(args), Some(repo)) => {
            update_preloaded::update_preloaded(&ctx, &app, &repo, args).await
        }
        (CommitGraphSubcommand::CommonBase(args), Some(repo)) => {
            common_base::common_base(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::SliceAncestors(args), Some(repo)) => {
            slice_ancestors::slice_ancestors(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::Children(args), Some(repo)) => {
            children::children(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::Descendants(args), Some(repo)) => {
            descendants::descendants(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::Segments(args), Some(repo)) => {
            segments::segments(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::IsAncestor(args), Some(repo)) => {
            is_ancestor::is_ancestor(&ctx, &repo, args).await
        }
        (CommitGraphSubcommand::UpdatePreloaded(sub_args), None) => {
            if args.all_repos {
                update_preloaded::update_preloaded_all_repos(&ctx, app, sub_args).await
            } else {
                Err(anyhow::anyhow!("Must specify a repo or use --all-repos"))
            }
        }
        (_, None) => Err(anyhow::anyhow!("Must specify a repo for this command")),
    }
}
