/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use insert::InsertArgs;
use map::MapArgs;
use metaconfig_types::RepoConfig;
use mononoke_app::args::SourceRepoArgs;
use mononoke_app::args::TargetRepoArgs;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use pushredirection::PushredirectionArgs;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use sql_query_config::SqlQueryConfig;
use verify_working_copy::VerifyWorkingCopyArgs;

mod insert;
mod map;
mod pushredirection;
mod verify_working_copy;

/// Query and manage cross repo syncs
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    source_repo: SourceRepoArgs,

    #[clap(flatten)]
    target_repo: TargetRepoArgs,

    #[clap(subcommand)]
    subcommand: CrossRepoSubcommand,
}

#[derive(Subcommand)]
pub enum CrossRepoSubcommand {
    Insert(InsertArgs),
    Map(MapArgs),
    Pushredirection(PushredirectionArgs),
    VerifyWorkingCopy(VerifyWorkingCopyArgs),
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    filenodes: dyn Filenodes,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    phases: dyn Phases,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    sql_query_config: SqlQueryConfig,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let source_repo = app.open_repo(&args.source_repo).await?;
    let target_repo = app.open_repo(&args.target_repo).await?;

    match args.subcommand {
        CrossRepoSubcommand::Map(args) => {
            map::map(&ctx, &app, source_repo, target_repo, args).await
        }
        CrossRepoSubcommand::Insert(args) => {
            insert::insert(&ctx, &app, source_repo, target_repo, args).await
        }
        CrossRepoSubcommand::Pushredirection(args) => {
            pushredirection::pushredirection(&ctx, &app, source_repo, target_repo, args).await
        }
        CrossRepoSubcommand::VerifyWorkingCopy(args) => {
            verify_working_copy::verify_working_copy(&ctx, &app, source_repo, target_repo, args)
                .await
        }
    }
}
