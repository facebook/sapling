/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod find_new;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use find_new::FindNewArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;

/// Query case conflicts for a commit
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: CaseConflictSubcommand,
}

#[derive(Subcommand)]
enum CaseConflictSubcommand {
    /// Find a case conflict in a commit that wasn't present in any of its parents.
    FindNew(FindNewArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bookmarks: dyn Bookmarks,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        CaseConflictSubcommand::FindNew(args) => find_new::find_new(&ctx, &repo, args).await,
    }
}
