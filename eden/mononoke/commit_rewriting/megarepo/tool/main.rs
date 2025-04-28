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
use cmdlib_x_repo::create_commit_syncer_from_matches;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::unsafe_sync_commit;
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
use slog::info;
use sql_query_config::SqlQueryConfig;

use crate::cli::COMMIT_BOOKMARK;
use crate::cli::COMMIT_HASH;
use crate::cli::GRADUAL_MERGE_PROGRESS;
use crate::cli::LAST_DELETION_COMMIT;
use crate::cli::PRE_DELETION_COMMIT;
use crate::cli::SYNC_COMMIT_AND_ANCESTORS;
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

async fn run_sync_commit_and_ancestors<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer =
        create_commit_syncer_from_matches::<cross_repo_sync::ConcreteRepo>(ctx, matches, None)
            .await?;

    let source_commit_hash = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not specified", COMMIT_HASH))?;

    let source_cs_id =
        helpers::csid_resolve(ctx, commit_syncer.get_source_repo(), source_commit_hash).await?;

    let (unsynced_ancestors, _) =
        find_toposorted_unsynced_ancestors(ctx, &commit_syncer, source_cs_id, None).await?;

    for ancestor in unsynced_ancestors {
        unsafe_sync_commit(
            ctx,
            ancestor,
            &commit_syncer,
            CandidateSelectionHint::Only,
            CommitSyncContext::AdminChangeMapping,
            None,
            false, // add_mapping_to_hg_extra
        )
        .await?;
    }

    let commit_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx, source_cs_id)
        .await?
        .ok_or_else(|| format_err!("was not able to remap a commit {}", source_cs_id))?;
    info!(ctx.logger(), "remapped to {:?}", commit_sync_outcome);

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
            (SYNC_COMMIT_AND_ANCESTORS, Some(sub_m)) => {
                run_sync_commit_and_ancestors(ctx, &matches, sub_m).await
            }
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
