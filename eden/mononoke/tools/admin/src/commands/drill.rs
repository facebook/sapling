/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod commit_throughput;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_throughput::CommitThroughputArgs;
use dbbookmarks::SqlBookmarks;
use filestore::FilestoreConfig;
use hook_manager::manager::HookManager;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mutable_renames::MutableRenames;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use restricted_paths::RestrictedPaths;

/// Run load drills against a repo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: DrillSubcommand,
}

#[derive(Subcommand)]
pub enum DrillSubcommand {
    /// Land many file-disjoint stacks in parallel onto a single test bookmark and measure commit throughput
    CommitThroughput(CommitThroughputArgs),
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    sql_bookmarks: SqlBookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    phases: dyn Phases,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    repo_permission_checker: dyn RepoPermissionChecker,

    #[facet]
    repo_lock: dyn RepoLock,

    #[facet]
    mutable_renames: MutableRenames,

    #[facet]
    hook_manager: HookManager,

    #[facet]
    restricted_paths: RestrictedPaths,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        DrillSubcommand::CommitThroughput(args) => {
            commit_throughput::commit_throughput(&ctx, &repo, args).await
        }
    }
}
