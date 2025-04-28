/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use anyhow::format_err;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use clap::ArgMatches;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use futures::future::try_join;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use sql_query_config::SqlQueryConfig;

use crate::cli::COMMIT_BOOKMARK;
use crate::cli::GRADUAL_MERGE_PROGRESS;
use crate::cli::LAST_DELETION_COMMIT;
use crate::cli::PRE_DELETION_COMMIT;
use crate::cli::setup_app;

mod cli;
mod gradual_merge;

#[derive(Clone)]
#[facet::container]
pub struct Repo(
    dyn BonsaiHgMapping,
    dyn BonsaiGitMapping,
    dyn BonsaiGlobalrevMapping,
    dyn PushrebaseMutationMapping,
    RepoCrossRepo,
    RepoBookmarkAttrs,
    dyn Bookmarks,
    dyn Phases,
    dyn BookmarkUpdateLog,
    FilestoreConfig,
    dyn MutableCounters,
    RepoBlobstore,
    RepoConfig,
    RepoDerivedData,
    RepoIdentity,
    CommitGraph,
    dyn CommitGraphWriter,
    dyn Filenodes,
    SqlQueryConfig,
);

async fn run_gradual_merge_progress<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or_else(|| format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or_else(|| format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or_else(|| format_err!("bookmark where to merge is not specified"))?;

    let last_deletion_commit = helpers::csid_resolve(ctx, &repo, last_deletion_commit);
    let pre_deletion_commit = helpers::csid_resolve(ctx, &repo, pre_deletion_commit);

    let (last_deletion_commit, pre_deletion_commit) =
        try_join(last_deletion_commit, pre_deletion_commit).await?;

    let (done, total) = gradual_merge::gradual_merge_progress(
        ctx,
        &repo,
        &pre_deletion_commit,
        &last_deletion_commit,
        &BookmarkKey::new(bookmark)?,
    )
    .await?;

    println!("{}/{}", done, total);

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = setup_app();
    let (matches, _runtime) = app.get_matches(fb)?;
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger_and_client_info(
        fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::MegarepoTool),
    );
    let ctx = &ctx;

    let subcommand_future = async {
        match matches.subcommand() {
            (GRADUAL_MERGE_PROGRESS, Some(sub_m)) => {
                run_gradual_merge_progress(ctx, &matches, sub_m).await
            }
            _ => bail!("oh no, wrong arguments provided!"),
        }
    };

    helpers::block_execute(
        subcommand_future,
        fb,
        "megarepotool",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
