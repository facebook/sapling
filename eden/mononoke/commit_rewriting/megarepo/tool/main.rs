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
use futures::future::try_join_all;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use regex::Regex;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use slog::info;
use sql_query_config::SqlQueryConfig;

use crate::cli::CATCHUP_VALIDATE_COMMAND;
use crate::cli::CHANGESET;
use crate::cli::COMMIT_BOOKMARK;
use crate::cli::COMMIT_HASH;
use crate::cli::GRADUAL_MERGE_PROGRESS;
use crate::cli::LAST_DELETION_COMMIT;
use crate::cli::MANUAL_COMMIT_SYNC;
use crate::cli::MAPPING_VERSION_NAME;
use crate::cli::PARENTS;
use crate::cli::PATH_REGEX;
use crate::cli::PRE_DELETION_COMMIT;
use crate::cli::SELECT_PARENTS_AUTOMATICALLY;
use crate::cli::SYNC_COMMIT_AND_ANCESTORS;
use crate::cli::TO_MERGE_CS_ID;
use crate::cli::setup_app;

mod catchup;
mod cli;
mod gradual_merge;
mod manual_commit_sync;

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

async fn run_manual_commit_sync<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches::<Repo>(ctx, matches, None).await?;

    let target_repo = commit_syncer.get_target_repo();
    let target_repo_parents = if sub_m.is_present(SELECT_PARENTS_AUTOMATICALLY) {
        None
    } else {
        let target_repo_parents = sub_m.values_of(PARENTS);
        match target_repo_parents {
            Some(target_repo_parents) => Some(
                try_join_all(
                    target_repo_parents
                        .into_iter()
                        .map(|p| helpers::csid_resolve(ctx, target_repo, p)),
                )
                .await?,
            ),
            None => Some(vec![]),
        }
    };

    let source_cs = sub_m
        .value_of(CHANGESET)
        .ok_or_else(|| format_err!("{} not set", CHANGESET))?;
    let source_repo = commit_syncer.get_source_repo();
    let source_cs_id = helpers::csid_resolve(ctx, source_repo, source_cs).await?;

    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("mapping-version-name is not specified"))?;

    let target_cs_id = manual_commit_sync::manual_commit_sync(
        ctx,
        &commit_syncer,
        source_cs_id,
        target_repo_parents,
        CommitSyncConfigVersion(mapping_version_name.to_string()),
    )
    .await?;
    info!(ctx.logger(), "target cs id is {:?}", target_cs_id);
    Ok(())
}

async fn run_catchup_validate<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, &ctx.logger().clone(), matches)
            .await?;

    let result_commit = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not set", COMMIT_HASH))?;
    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let result_commit = helpers::csid_resolve(ctx, &repo, result_commit);

    let to_merge_cs_id = helpers::csid_resolve(ctx, &repo, to_merge_cs_id);

    let (result_commit, to_merge_cs_id) = try_join(result_commit, to_merge_cs_id).await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    catchup::validate(ctx, &repo, result_commit, to_merge_cs_id, path_regex).await?;

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
            (MANUAL_COMMIT_SYNC, Some(sub_m)) => run_manual_commit_sync(ctx, &matches, sub_m).await,
            (SYNC_COMMIT_AND_ANCESTORS, Some(sub_m)) => {
                run_sync_commit_and_ancestors(ctx, &matches, sub_m).await
            }

            // All commands relevant to gradual merge
            (CATCHUP_VALIDATE_COMMAND, Some(sub_m)) => {
                run_catchup_validate(ctx, &matches, sub_m).await
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
