/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "4522397"]
use anyhow::{format_err, Error};
use backsyncer::{backsync_latest, open_backsyncer_dbs, BacksyncLimit, TargetRepoDbs};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use borrowed::borrowed;
use clap::ArgMatches;
use cmdlib::args;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use cross_repo_sync::{
    create_commit_syncers, rewrite_commit, CandidateSelectionHint, CommitSyncer, Syncers,
};
use derived_data_utils::derived_data_utils;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{self, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use import_tools::{GitimportPreferences, GitimportTarget};
use itertools::Itertools;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use manifest::ManifestOps;
use maplit::hashset;
use mercurial_types::{HgChangesetId, MPath};
use metaconfig_types::RepoConfig;
use mononoke_hg_sync_job_helper_lib::wait_for_latest_log_id_to_be_synced;
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime};
use movers::{DefaultAction, Mover};
use mutable_counters::SqlMutableCounters;
use pushrebase::do_pushrebase_bonsai;
use serde::{Deserialize, Serialize};
use serde_json;
use slog::info;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    process, time,
};
use topo_sort::sort_topological;
use unbundle::get_pushrebase_hooks;

mod cli;
mod tests;

use crate::cli::{
    setup_app, setup_import_args, ARG_BOOKMARK_SUFFIX, ARG_DEST_BOOKMARK, ARG_PHAB_CHECK_DISABLED,
    CHECK_ADDITIONAL_SETUP_STEPS, IMPORT, RECOVER_PROCESS, SAVED_RECOVERY_FILE_PATH,
};

#[derive(Deserialize, Clone, Debug)]
struct GraphqlQueryObj {
    differential_commit_query: Vec<GraphqlCommitQueryObj>,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlCommitQueryObj {
    results: GraphqlResultsObj,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlResultsObj {
    nodes: Vec<GraphqlImportedObj>,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlImportedObj {
    imported: bool,
}
#[derive(Debug, Serialize)]
struct GraphqlInputVariables {
    commit: String,
}
#[derive(Debug)]
struct CheckerFlags {
    phab_check_disabled: bool,
    x_repo_check_disabled: bool,
    hg_sync_check_disabled: bool,
}
#[derive(Clone, Debug)]
struct ChangesetArgs {
    pub author: String,
    pub message: String,
    pub datetime: DateTime,
}
#[derive(Clone, Debug, PartialEq)]
struct RepoImportSetting {
    importing_bookmark: BookmarkName,
    dest_bookmark: BookmarkName,
}

#[derive(Clone)]
struct SmallRepoBackSyncVars {
    large_to_small_syncer: CommitSyncer<SqlSyncedCommitMapping>,
    target_repo_dbs: TargetRepoDbs,
    small_repo_bookmark: BookmarkName,
    small_repo: BlobRepo,
    maybe_call_sign: Option<String>,
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug, PartialEq)]
enum ImportStage {
    GitImport,
    RewritePaths,
    DeriveBonsais,
    MoveBookmark,
    MergeCommits,
    PushCommit,
}

/*
    Most fields can be found with 'repo_import --help'
*/
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RecoveryFields {
    /// Indicates which stage we will recover from in case of recovery process
    import_stage: ImportStage,
    recovery_file_path: String,
    git_repo_path: String,
    dest_path: String,
    bookmark_suffix: String,
    batch_size: usize,
    move_bookmark_commits_done: usize,
    phab_check_disabled: bool,
    x_repo_check_disabled: bool,
    hg_sync_check_disabled: bool,
    sleep_time: u64,
    dest_bookmark_name: String,
    commit_author: String,
    commit_message: String,
    datetime: DateTime,
    /// ChangesetId of the merged commit we make to merge the imported commits into dest_bookmark
    merged_cs_id: Option<ChangesetId>,
    /// ChangesetIds created after shifting the file paths of the gitimported commits
    shifted_bcs_ids: Option<Vec<ChangesetId>>,
    /// ChangesetIds of the gitimported commits
    gitimport_bcs_ids: Option<Vec<ChangesetId>>,
}

async fn rewrite_file_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mover: &Mover,
    gitimport_bcs_ids: &[ChangesetId],
) -> Result<Vec<ChangesetId>, Error> {
    let mut remapped_parents: HashMap<ChangesetId, ChangesetId> = HashMap::new();
    let mut bonsai_changesets = vec![];

    let len = gitimport_bcs_ids.len();
    let gitimport_changesets = stream::iter(gitimport_bcs_ids.iter().map(|bcs_id| async move {
        let bcs = bcs_id.load(ctx.clone(), &repo.get_blobstore()).await?;
        Result::<_, Error>::Ok(bcs)
    }))
    .buffered(len)
    .try_collect::<Vec<_>>()
    .await?;

    for (index, bcs) in gitimport_changesets.iter().enumerate() {
        let bcs_id = bcs.get_changeset_id();
        let rewritten_bcs_opt = rewrite_commit(
            ctx.clone(),
            bcs.clone().into_mut(),
            &remapped_parents,
            mover.clone(),
            repo.clone(),
        )
        .await?;

        if let Some(rewritten_bcs_mut) = rewritten_bcs_opt {
            let rewritten_bcs = rewritten_bcs_mut.freeze()?;
            let rewritten_bcs_id = rewritten_bcs.get_changeset_id();
            remapped_parents.insert(bcs_id.clone(), rewritten_bcs_id);
            info!(
                ctx.logger(),
                "Commit {}/{}: Remapped {:?} => {:?}",
                (index + 1),
                len,
                bcs_id,
                rewritten_bcs_id,
            );
            bonsai_changesets.push(rewritten_bcs);
        }
    }

    bonsai_changesets = sort_bcs(&bonsai_changesets)?;
    let bcs_ids = get_cs_ids(&bonsai_changesets);
    info!(ctx.logger(), "Saving shifted bonsai changesets");
    save_bonsai_changesets(bonsai_changesets, ctx.clone(), repo.clone())
        .compat()
        .await?;
    info!(ctx.logger(), "Saved shifted bonsai changesets");
    Ok(bcs_ids)
}

async fn back_sync_commits_to_small_repo(
    ctx: &CoreContext,
    small_repo: &BlobRepo,
    large_to_small_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
    bcs_ids: &[ChangesetId],
) -> Result<Vec<ChangesetId>, Error> {
    info!(
        ctx.logger(),
        "Back syncing from large repo {} to small repo {}",
        large_to_small_syncer.get_large_repo().name(),
        small_repo.name()
    );
    let mut synced_bcs_ids = vec![];
    for bcs_id in bcs_ids {
        // It is always safe to use `CandidateSelectionHint::Only` in
        // the large-to-small direction
        let maybe_synced_cs_id: Option<ChangesetId> = large_to_small_syncer
            .sync_commit(&ctx, bcs_id.clone(), CandidateSelectionHint::Only)
            .await?;
        if let Some(synced_cs_id) = maybe_synced_cs_id {
            info!(
                ctx.logger(),
                "Synced large repo cs: {} => {}", bcs_id, synced_cs_id
            );
            synced_bcs_ids.push(synced_cs_id);
        }
    }

    info!(ctx.logger(), "Finished back syncing shifted bonsais");
    Ok(synced_bcs_ids)
}

async fn derive_bonsais_single_repo(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_ids: &[ChangesetId],
) -> Result<(), Error> {
    let derived_data_types = &repo.get_derived_data_config().derived_data_types;

    let len = derived_data_types.len();
    let mut derived_utils = vec![];
    for ty in derived_data_types {
        let utils = derived_data_utils(repo.clone(), ty)?;
        derived_utils.push(utils);
    }

    stream::iter(derived_utils)
        .map(Ok)
        .try_for_each_concurrent(len, |derived_util| async move {
            for csid in bcs_ids {
                derived_util
                    .derive(ctx.clone(), repo.clone(), csid.clone())
                    .map_ok(|_| ())
                    .await?;
            }
            Result::<(), Error>::Ok(())
        })
        .await
}

async fn move_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    shifted_bcs_ids: &[ChangesetId],
    bookmark: &BookmarkName,
    checker_flags: &CheckerFlags,
    maybe_call_sign: &Option<String>,
    mutable_counters: &SqlMutableCounters,
    maybe_small_repo_back_sync_vars: &Option<SmallRepoBackSyncVars>,
    recovery_fields: &mut RecoveryFields,
) -> Result<(), Error> {
    let batch_size = recovery_fields.batch_size;
    let sleep_time = recovery_fields.sleep_time;
    info!(ctx.logger(), "Start moving the bookmark");
    if shifted_bcs_ids.is_empty() {
        return Err(format_err!("There is no bonsai changeset present"));
    }

    let first_csid = match shifted_bcs_ids.first() {
        Some(first) => first,
        None => {
            return Err(format_err!("There is no bonsai changeset present"));
        }
    };

    let maybe_old_csid = repo
        .get_bonsai_bookmark(ctx.clone(), bookmark)
        .compat()
        .await?;

    /* If the bookmark already exists, we should continue moving the
    bookmark from the last commit it points to */
    let mut old_csid = match maybe_old_csid {
        Some(ref id) => id,
        None => first_csid,
    };

    let mut transaction = repo.update_bookmark_transaction(ctx.clone());
    if maybe_old_csid.is_none() {
        transaction.create(
            &bookmark,
            old_csid.clone(),
            BookmarkUpdateReason::ManualMove,
            None,
        )?;
        if !transaction.commit().await? {
            return Err(format_err!("Logical failure while creating {:?}", bookmark));
        }
        info!(
            ctx.logger(),
            "Created bookmark {:?} pointing to {}", bookmark, old_csid
        );
    }

    let commits_done = recovery_fields.move_bookmark_commits_done;

    for chunk in shifted_bcs_ids
        .iter()
        .skip(commits_done)
        .enumerate()
        .chunks(batch_size)
        .into_iter()
    {
        transaction = repo.update_bookmark_transaction(ctx.clone());
        let (shifted_index, curr_csid) = match chunk.last() {
            Some(tuple) => tuple,
            None => {
                return Err(format_err!("There is no bonsai changeset present"));
            }
        };
        transaction.update(
            &bookmark,
            curr_csid.clone(),
            old_csid.clone(),
            BookmarkUpdateReason::ManualMove,
            None,
        )?;

        if !transaction.commit().await? {
            return Err(format_err!("Logical failure while setting {:?}", bookmark));
        }
        info!(
            ctx.logger(),
            "Set bookmark {:?} to point to {:?}", bookmark, curr_csid
        );

        recovery_fields.move_bookmark_commits_done = commits_done + shifted_index;

        let check_repo = async move {
            let hg_csid = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), curr_csid.clone())
                .compat()
                .await?;
            check_dependent_systems(
                &ctx,
                &repo,
                &checker_flags,
                hg_csid,
                sleep_time,
                &mutable_counters,
                &maybe_call_sign,
            )
            .await?;
            Result::<_, Error>::Ok(())
        };

        let check_small_repo = async move {
            let small_repo_back_sync_vars = match maybe_small_repo_back_sync_vars {
                Some(v) => v,
                None => return Ok(()),
            };

            info!(ctx.logger(), "Back syncing bookmark movement to small repo");
            backsync_latest(
                ctx.clone(),
                small_repo_back_sync_vars.large_to_small_syncer.clone(),
                small_repo_back_sync_vars.target_repo_dbs.clone(),
                BacksyncLimit::NoLimit,
            )
            .await?;
            let small_repo_cs_id = repo
                .get_bonsai_bookmark(ctx.clone(), &small_repo_back_sync_vars.small_repo_bookmark)
                .compat()
                .await?
                .ok_or_else(|| {
                    format_err!(
                        "Couldn't extract backsynced changeset id from bookmark: {}",
                        small_repo_back_sync_vars.small_repo_bookmark
                    )
                })?;

            let small_repo_hg_csid = small_repo_back_sync_vars
                .small_repo
                .get_hg_from_bonsai_changeset(ctx.clone(), small_repo_cs_id)
                .compat()
                .await?;

            check_dependent_systems(
                &ctx,
                &small_repo_back_sync_vars.small_repo,
                &checker_flags,
                small_repo_hg_csid,
                sleep_time,
                &mutable_counters,
                &small_repo_back_sync_vars.maybe_call_sign,
            )
            .await?;

            Result::<_, Error>::Ok(())
        };

        future::try_join(
            check_repo.map_err(|e| e.context("Error checking dependent systems")),
            check_small_repo
                .map_err(|e| e.context("Error checking dependent systems in small repository")),
        )
        .await?;
        old_csid = curr_csid;
    }
    info!(ctx.logger(), "Finished moving the bookmark");
    Ok(())
}

async fn merge_imported_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    imported_cs_id: ChangesetId,
    dest_bookmark: &BookmarkName,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    info!(
        ctx.logger(),
        "Merging the imported commits into given bookmark, {}", dest_bookmark
    );
    let master_cs_id = match repo
        .get_bonsai_bookmark(ctx.clone(), dest_bookmark)
        .compat()
        .await?
    {
        Some(id) => id,
        None => {
            return Err(format_err!(
                "Couldn't extract changeset id from bookmark: {}",
                dest_bookmark
            ));
        }
    };
    let master_leaf_entries = get_leaf_entries(&ctx, &repo, master_cs_id).await?;

    let imported_leaf_entries = get_leaf_entries(&ctx, &repo, imported_cs_id).await?;

    let intersection: Vec<MPath> = imported_leaf_entries
        .intersection(&master_leaf_entries)
        .cloned()
        .collect();

    if !intersection.is_empty() {
        return Err(format_err!(
            "There are paths present in both parents: {:?} ...",
            intersection
        ));
    }

    info!(ctx.logger(), "Done checking path conflicts");

    info!(
        ctx.logger(),
        "Creating a merge bonsai changeset with parents: {}, {}", master_cs_id, imported_cs_id
    );

    let ChangesetArgs {
        author,
        message,
        datetime,
    } = changeset_args;

    let merged_cs = BonsaiChangesetMut {
        parents: vec![master_cs_id, imported_cs_id],
        author: author.clone(),
        author_date: datetime,
        committer: Some(author.to_string()),
        committer_date: Some(datetime),
        message: message.to_string(),
        extra: BTreeMap::new(),
        file_changes: BTreeMap::new(),
    }
    .freeze()?;

    let merged_cs_id = merged_cs.get_changeset_id();
    info!(
        ctx.logger(),
        "Created merge bonsai: {} and changeset: {:?}", merged_cs_id, merged_cs
    );

    save_bonsai_changesets(vec![merged_cs], ctx.clone(), repo.clone())
        .compat()
        .await?;
    info!(ctx.logger(), "Finished merging");
    Ok(merged_cs_id)
}

async fn push_merge_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    merged_cs_id: ChangesetId,
    bookmark_to_merge_into: &BookmarkName,
    repo_config: &RepoConfig,
) -> Result<ChangesetId, Error> {
    info!(ctx.logger(), "Running pushrebase");

    let merged_cs = merged_cs_id.load(ctx.clone(), repo.blobstore()).await?;
    let pushrebase_flags = repo_config.pushrebase.flags;
    let pushrebase_hooks = get_pushrebase_hooks(&repo, &repo_config.pushrebase);

    let pushrebase_res = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &pushrebase_flags,
        bookmark_to_merge_into,
        &hashset![merged_cs],
        None,
        &pushrebase_hooks,
    )
    .await?;

    let pushrebase_cs_id = pushrebase_res.head;
    info!(
        ctx.logger(),
        "Finished pushrebasing to {}", pushrebase_cs_id
    );
    Ok(pushrebase_cs_id)
}

async fn get_leaf_entries(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashSet<MPath>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;
    let hg_cs = hg_cs_id.load(ctx.clone(), &repo.get_blobstore()).await?;
    hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .map_ok(|(path, (_file_type, _filenode_id))| path)
        .try_collect::<HashSet<_>>()
        .await
}

async fn check_dependent_systems(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker_flags: &CheckerFlags,
    hg_csid: HgChangesetId,
    sleep_time: u64,
    mutable_counters: &SqlMutableCounters,
    maybe_call_sign: &Option<String>,
) -> Result<(), Error> {
    // if a check is disabled, we have already passed the check
    let mut passed_phab_check = checker_flags.phab_check_disabled;
    let mut _passed_x_repo_check = checker_flags.x_repo_check_disabled;
    let passed_hg_sync_check = checker_flags.hg_sync_check_disabled;

    while !passed_phab_check {
        let call_sign = maybe_call_sign.as_ref().unwrap();
        passed_phab_check = phabricator_commit_check(&call_sign, &hg_csid).await?;
        if !passed_phab_check {
            info!(
                ctx.logger(),
                "Phabricator hasn't parsed commit: {:?}", hg_csid
            );
            time::delay_for(time::Duration::from_secs(sleep_time)).await;
        }
    }

    if !passed_hg_sync_check {
        wait_for_latest_log_id_to_be_synced(ctx, repo, mutable_counters, sleep_time).await?;
    }

    Ok(())
}

async fn phabricator_commit_check(call_sign: &str, hg_csid: &HgChangesetId) -> Result<bool, Error> {
    let commit_id = format!("r{}{}", call_sign, hg_csid);
    let query = "query($commit: String!) {
                    differential_commit_query(query_params:{commits:[$commit]}) {
                        results {
                            nodes {
                                imported
                            }
                        }
                    }
                }";
    let variables = serde_json::to_string(&GraphqlInputVariables { commit: commit_id }).unwrap();
    let output = process::Command::new("jf")
        .arg("graphql")
        .arg("--query")
        .arg(query)
        .arg("--variables")
        .arg(variables)
        .output()
        .await?;
    if !output.status.success() {
        let e = format_err!(
            "Failed to fetch graphql commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(e);
    }
    let query: GraphqlQueryObj = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    let first_query = match query.differential_commit_query.first() {
        Some(first) => first,
        None => {
            return Err(format_err!(
                "No results were found when checking phabricator"
            ));
        }
    };
    let nodes = &first_query.results.nodes;
    let imported = match nodes.first() {
        Some(imp_obj) => imp_obj.imported,
        None => return Ok(false),
    };
    Ok(imported)
}

fn is_valid_bookmark_suffix(bookmark_suffix: &str) -> bool {
    let spec_chars = "./-_";
    bookmark_suffix
        .chars()
        .all(|c| c.is_alphanumeric() || spec_chars.contains(c))
}

fn sort_bcs(shifted_bcs: &[BonsaiChangeset]) -> Result<Vec<BonsaiChangeset>, Error> {
    let mut bcs_parents = HashMap::new();
    let mut id_bcs = HashMap::new();
    for bcs in shifted_bcs {
        let parents: Vec<_> = bcs.parents().collect();
        let bcs_id = bcs.get_changeset_id();
        bcs_parents.insert(bcs_id, parents);
        id_bcs.insert(bcs_id, bcs);
    }

    let sorted_commits = sort_topological(&bcs_parents).expect("loop in commit chain!");
    let mut sorted_bcs: Vec<BonsaiChangeset> = vec![];
    for csid in sorted_commits {
        match id_bcs.get(&csid) {
            Some(&bcs) => sorted_bcs.push(bcs.clone()),
            _ => {
                return Err(format_err!(
                    "Could not find mapping for changeset id {}",
                    csid
                ));
            }
        }
    }
    Ok(sorted_bcs)
}

fn get_cs_ids(changesets: &[BonsaiChangeset]) -> Vec<ChangesetId> {
    let mut cs_ids = vec![];
    for bcs in changesets {
        cs_ids.push(bcs.get_changeset_id());
    }
    cs_ids
}

fn get_importing_bookmark(bookmark_suffix: &str) -> Result<BookmarkName, Error> {
    BookmarkName::new(format!("repo_import_{}", &bookmark_suffix))
}

// Note: pushredirection only works from small repo to large repo.
async fn get_large_repo_config_if_pushredirected<'a>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    live_commit_sync_config: &CfgrLiveCommitSyncConfig,
    repos: &HashMap<String, RepoConfig>,
) -> Result<Option<RepoConfig>, Error> {
    let repo_id = repo.get_repoid();
    let enabled = live_commit_sync_config.push_redirector_enabled_for_public(repo_id);

    if enabled {
        let commit_sync_config =
            match live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_id) {
                Ok(config) => config,
                Err(e) => return Err(format_err!("Failed to fetch commit sync config: {}", e)),
            };
        let large_repo_id = commit_sync_config.large_repo_id;
        let (_, large_repo_config) = match repos
            .iter()
            .find(|(_, repo_config)| repo_config.repoid == large_repo_id)
        {
            Some(result) => result,
            None => {
                return Err(format_err!(
                    "Couldn't fetch the large repo config we pushredirect into"
                ));
            }
        };
        return Ok(Some(large_repo_config.clone()));
    }
    Ok(None)
}

async fn get_large_repo_setting<M>(
    ctx: &CoreContext,
    small_repo_setting: &RepoImportSetting,
    commit_syncer: &CommitSyncer<M>,
) -> Result<RepoImportSetting, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    info!(
        ctx.logger(),
        "Generating variables to import into large repo"
    );

    let RepoImportSetting {
        importing_bookmark,
        dest_bookmark,
    } = small_repo_setting;

    let large_importing_bookmark =
        commit_syncer
            .rename_bookmark(ctx, &importing_bookmark)?
            .ok_or_else(|| format_err!(
        "Bookmark {:?} unexpectedly dropped in {:?} when trying to generate large_importing_bookmark",
        importing_bookmark,
        commit_syncer
    ))?;
    info!(
        ctx.logger(),
        "Set large repo's importing bookmark to {}", large_importing_bookmark
    );
    let large_dest_bookmark = commit_syncer
        .rename_bookmark(ctx, &dest_bookmark)?
        .ok_or_else(|| {
            format_err!(
        "Bookmark {:?} unexpectedly dropped in {:?} when trying to generate large_dest_bookmark",
        dest_bookmark,
        commit_syncer
    )
        })?;
    info!(
        ctx.logger(),
        "Set large repo's destination bookmark to {}", large_dest_bookmark
    );

    let large_repo_setting = RepoImportSetting {
        importing_bookmark: large_importing_bookmark,
        dest_bookmark: large_dest_bookmark,
    };
    info!(ctx.logger(), "Finished generating the variables");
    Ok(large_repo_setting)
}

async fn get_pushredirected_vars(
    ctx: &CoreContext,
    repo: &BlobRepo,
    repo_import_setting: &RepoImportSetting,
    large_repo_config: &RepoConfig,
    matches: &ArgMatches<'_>,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(BlobRepo, RepoImportSetting, Syncers<SqlSyncedCommitMapping>), Error> {
    let large_repo_id = large_repo_config.repoid;
    let large_repo = args::open_repo_with_repo_id(ctx.fb, &ctx.logger(), large_repo_id, &matches)
        .compat()
        .await?;
    let commit_sync_config = match large_repo_config.commit_sync_config.clone() {
        Some(config) => config,
        None => {
            return Err(format_err!(
                "The repo ({}) doesn't have a commit sync config",
                large_repo.name()
            ));
        }
    };

    if commit_sync_config.small_repos.len() > 1 {
        return Err(format_err!(
            "Currently repo_import tool doesn't support backsyncing into multiple small repos for large repo {:?}, name: {}",
            large_repo_id,
            large_repo.name()
        ));
    }
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(ctx.fb, &matches)
        .compat()
        .await?;
    let syncers = create_commit_syncers(
        repo.clone(),
        large_repo.clone(),
        mapping.clone(),
        Arc::new(live_commit_sync_config),
    )?;

    let large_repo_import_setting =
        get_large_repo_setting(&ctx, &repo_import_setting, &syncers.small_to_large).await?;
    Ok((large_repo, large_repo_import_setting, syncers))
}

async fn save_importing_state(recovery_fields: &RecoveryFields) -> Result<(), Error> {
    let mut proc_recovery_file = fs::File::create(&recovery_fields.recovery_file_path).await?;
    let serialized = serde_json::to_string_pretty(&recovery_fields)?;
    proc_recovery_file.write_all(serialized.as_bytes()).await?;
    Ok(())
}

async fn fetch_recovery_state(
    ctx: &CoreContext,
    saved_recovery_file_paths: &str,
) -> Result<RecoveryFields, Error> {
    info!(ctx.logger(), "Fetching the recovery stage for importing");
    let mut saved_proc_recovery_file = fs::File::open(saved_recovery_file_paths).await?;
    let mut serialized = String::new();
    saved_proc_recovery_file
        .read_to_string(&mut serialized)
        .await?;
    let recovery_fields: RecoveryFields = serde_json::from_str(&serialized)?;
    info!(
        ctx.logger(),
        "Fetched the recovery stage for importing.\nStarting from stage: {:?}",
        recovery_fields.import_stage
    );
    Ok(recovery_fields)
}

async fn repo_import(
    ctx: CoreContext,
    mut repo: BlobRepo,
    recovery_fields: &mut RecoveryFields,
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
) -> Result<(), Error> {
    let arg_git_repo_path = recovery_fields.git_repo_path.clone();
    let path = Path::new(&arg_git_repo_path);
    let dest_path_prefix = MPath::new(&recovery_fields.dest_path)?;
    let importing_bookmark = get_importing_bookmark(&recovery_fields.bookmark_suffix)?;
    if !is_valid_bookmark_suffix(&recovery_fields.bookmark_suffix) {
        return Err(format_err!(
            "The bookmark suffix contains invalid character(s).
                    You can only use alphanumeric and \"./-_\" characters"
        ));
    }
    let config_store = args::init_config_store(fb, ctx.logger(), &matches)?;
    let dest_bookmark = BookmarkName::new(&recovery_fields.dest_bookmark_name)?;
    let changeset_args = ChangesetArgs {
        author: recovery_fields.commit_author.clone(),
        message: recovery_fields.commit_message.clone(),
        datetime: recovery_fields.datetime,
    };
    let mut repo_import_setting = RepoImportSetting {
        importing_bookmark,
        dest_bookmark,
    };
    let (_, mut repo_config) = args::get_config_by_repoid(&matches, repo.get_repoid())?;
    let mut call_sign = repo_config.phabricator_callsign.clone();
    if !recovery_fields.phab_check_disabled && call_sign.is_none() {
        return Err(format_err!(
            "The repo ({}) we import to doesn't have a callsign. \
                     Make sure the callsign for the repo is set in configerator: \
                     e.g CF/../source/scm/mononoke/repos/repos/hg.cinc",
            repo.name()
        ));
    }
    let checker_flags = CheckerFlags {
        phab_check_disabled: recovery_fields.phab_check_disabled,
        x_repo_check_disabled: recovery_fields.x_repo_check_disabled,
        hg_sync_check_disabled: recovery_fields.hg_sync_check_disabled,
    };
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;

    let configs = args::load_repo_configs(&matches)?;
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    let maybe_large_repo_config = get_large_repo_config_if_pushredirected(
        &ctx,
        &repo,
        &live_commit_sync_config,
        &configs.repos,
    )
    .await?;
    let mut maybe_small_repo_back_sync_vars = None;
    let mut movers = vec![movers::mover_factory(
        HashMap::new(),
        DefaultAction::PrependPrefix(dest_path_prefix),
    )?];

    if let Some(large_repo_config) = maybe_large_repo_config {
        let (large_repo, large_repo_import_setting, syncers) = get_pushredirected_vars(
            &ctx,
            &repo,
            &repo_import_setting,
            &large_repo_config,
            &matches,
            live_commit_sync_config,
        )
        .await?;
        let target_repo_dbs = open_backsyncer_dbs(
            ctx.clone(),
            repo.clone(),
            repo_config.storage_config.metadata,
            mysql_options,
            readonly_storage,
        )
        .await?;
        maybe_small_repo_back_sync_vars = Some(SmallRepoBackSyncVars {
            large_to_small_syncer: syncers.large_to_small.clone(),
            target_repo_dbs,
            small_repo_bookmark: repo_import_setting.importing_bookmark.clone(),
            small_repo: repo.clone(),
            maybe_call_sign: call_sign.clone(),
        });

        movers.push(syncers.small_to_large.get_current_mover_DEPRECATED(&ctx)?);
        repo_import_setting = large_repo_import_setting;
        repo = large_repo;
        repo_config = large_repo_config;
        call_sign = repo_config.phabricator_callsign.clone();
        if !recovery_fields.phab_check_disabled && call_sign.is_none() {
            return Err(format_err!(
                "Repo ({}) we push-redirect to doesn't have a callsign. \
                         Make sure the callsign for the repo is set in configerator: \
                         e.g CF/../source/scm/mononoke/repos/repos/hg.cinc",
                repo.name()
            ));
        }
    }

    let combined_mover: Mover = Arc::new(move |source_path: &MPath| {
        let mut mutable_path = source_path.clone();
        for mover in movers.clone() {
            let maybe_path = mover(&mutable_path)?;
            mutable_path = match maybe_path {
                Some(moved_path) => moved_path,
                None => return Ok(None),
            };
        }
        Ok(Some(mutable_path))
    });

    let mutable_counters = args::open_sql::<SqlMutableCounters>(ctx.fb, &matches)
        .compat()
        .await?;

    // Importing process starts here
    if recovery_fields.import_stage == ImportStage::GitImport {
        let prefs = GitimportPreferences::default();
        let target = GitimportTarget::FullRepo;
        info!(ctx.logger(), "Started importing git commits to Mononoke");
        let import_map = import_tools::gitimport(&ctx, &repo, &path, target, prefs).await?;
        info!(ctx.logger(), "Added commits to Mononoke");

        let bonsai_values: Vec<(ChangesetId, BonsaiChangeset)> =
            import_map.values().cloned().collect();
        let gitimport_bcs: Vec<BonsaiChangeset> =
            bonsai_values.iter().map(|(_, bcs)| bcs.clone()).collect();
        let gitimport_bcs_ids: Vec<ChangesetId> =
            bonsai_values.iter().map(|(id, _)| id.clone()).collect();

        info!(ctx.logger(), "Saving gitimported bonsai changesets");
        save_bonsai_changesets(gitimport_bcs.clone(), ctx.clone(), repo.clone())
            .compat()
            .await?;
        info!(ctx.logger(), "Saved gitimported bonsai changesets");

        recovery_fields.import_stage = ImportStage::RewritePaths;
        recovery_fields.gitimport_bcs_ids = Some(gitimport_bcs_ids);
        save_importing_state(&recovery_fields).await?;
    }

    if recovery_fields.import_stage == ImportStage::RewritePaths {
        let gitimport_bcs_ids = recovery_fields
            .gitimport_bcs_ids
            .as_ref()
            .ok_or_else(|| format_err!("gitimported changeset ids are not found"))?;
        let shifted_bcs_ids =
            rewrite_file_paths(&ctx, &repo, &combined_mover, &gitimport_bcs_ids).await?;
        recovery_fields.import_stage = ImportStage::DeriveBonsais;
        recovery_fields.shifted_bcs_ids = Some(shifted_bcs_ids);
        save_importing_state(&recovery_fields).await?;
    }

    let shifted_bcs_ids = recovery_fields
        .shifted_bcs_ids
        .as_ref()
        .ok_or_else(|| format_err!("Changeset ids after rewriting the file paths are not found"))?
        .clone();

    if recovery_fields.import_stage == ImportStage::DeriveBonsais {
        let derive_changesets = derive_bonsais_single_repo(&ctx, &repo, &shifted_bcs_ids);

        let backsync_and_derive_changesets = {
            borrowed!(ctx, shifted_bcs_ids, maybe_small_repo_back_sync_vars);

            async move {
                let vars = match maybe_small_repo_back_sync_vars {
                    Some(vars) => {
                        info!(ctx.logger(), "Backsyncing changesets");
                        vars
                    }
                    None => return Ok(()),
                };

                let small_repo = &vars.small_repo;

                let synced_bcs_ids = back_sync_commits_to_small_repo(
                    &ctx,
                    &small_repo,
                    &vars.large_to_small_syncer,
                    &shifted_bcs_ids,
                )
                .await?;

                derive_bonsais_single_repo(&ctx, &small_repo, &synced_bcs_ids).await?;
                Ok(())
            }
        };

        info!(ctx.logger(), "Start deriving data types");
        future::try_join(derive_changesets, backsync_and_derive_changesets).await?;
        info!(ctx.logger(), "Finished deriving data types");

        recovery_fields.import_stage = ImportStage::MoveBookmark;
        save_importing_state(&recovery_fields).await?;
    }

    if recovery_fields.import_stage == ImportStage::MoveBookmark {
        move_bookmark(
            &ctx,
            &repo,
            &shifted_bcs_ids,
            &repo_import_setting.importing_bookmark,
            &checker_flags,
            &call_sign,
            &mutable_counters,
            &maybe_small_repo_back_sync_vars,
            recovery_fields,
        )
        .await?;

        recovery_fields.import_stage = ImportStage::MergeCommits;
        save_importing_state(&recovery_fields).await?;
    }

    if recovery_fields.import_stage == ImportStage::MergeCommits {
        let imported_cs_id = match shifted_bcs_ids.last() {
            Some(bcs_id) => bcs_id,
            None => return Err(format_err!("There is no bonsai changeset present")),
        };

        let maybe_merged_cs_id = Some(
            merge_imported_commit(
                &ctx,
                &repo,
                imported_cs_id.clone(),
                &repo_import_setting.dest_bookmark,
                changeset_args,
            )
            .await?,
        );

        recovery_fields.import_stage = ImportStage::PushCommit;
        recovery_fields.merged_cs_id = maybe_merged_cs_id;
        save_importing_state(&recovery_fields).await?;
    }

    let merged_cs_id = recovery_fields
        .merged_cs_id
        .ok_or_else(|| format_err!("Changeset id for the merged commit is not found"))?;
    push_merge_commit(
        &ctx,
        &repo,
        merged_cs_id,
        &repo_import_setting.dest_bookmark,
        &repo_config,
    )
    .await?;
    Ok(())
}

async fn check_additional_setup_steps(
    ctx: CoreContext,
    repo: BlobRepo,
    fb: FacebookInit,
    sub_arg_matches: &ArgMatches<'_>,
    matches: &ArgMatches<'_>,
) -> Result<(), Error> {
    let bookmark_suffix = match sub_arg_matches.value_of(ARG_BOOKMARK_SUFFIX) {
        Some(suffix) => suffix,
        None => {
            return Err(format_err!(
                "Expected a bookmark suffix for checking additional setup steps"
            ));
        }
    };
    if !is_valid_bookmark_suffix(&bookmark_suffix) {
        return Err(format_err!(
            "The bookmark suffix contains invalid character(s).
                    You can only use alphanumeric and \"./-_\" characters"
        ));
    }
    let importing_bookmark = get_importing_bookmark(&bookmark_suffix)?;
    info!(
        ctx.logger(),
        "The importing bookmark name is: {}. \
        Make sure to notify Phabricator oncall to track this bookmark!",
        importing_bookmark
    );
    let dest_bookmark_name = match sub_arg_matches.value_of(ARG_DEST_BOOKMARK) {
        Some(name) => name,
        None => {
            return Err(format_err!(
                "Expected a destination bookmark name for checking additional setup steps"
            ));
        }
    };
    let dest_bookmark = BookmarkName::new(dest_bookmark_name)?;
    info!(
        ctx.logger(),
        "The destination bookmark name is: {}. \
        If the bookmark doesn't exist already, make sure to notify Phabricator oncall to track it!",
        dest_bookmark
    );

    let repo_import_setting = RepoImportSetting {
        importing_bookmark,
        dest_bookmark,
    };
    let (_, repo_config) = args::get_config_by_repoid(&matches, repo.get_repoid())?;

    let call_sign = repo_config.phabricator_callsign;
    let phab_check_disabled = sub_arg_matches.is_present(ARG_PHAB_CHECK_DISABLED);
    if !phab_check_disabled && call_sign.is_none() {
        return Err(format_err!(
            "The repo ({}) we import to doesn't have a callsign for checking the commits on Phabricator. \
                     Make sure the callsign for the repo is set in configerator: \
                     e.g CF/../source/scm/mononoke/repos/repos/hg.cinc",
            repo.name()
        ));
    }

    let config_store = args::init_config_store(fb, ctx.logger(), &matches)?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;
    let configs = args::load_repo_configs(&matches)?;
    let maybe_large_repo_config = get_large_repo_config_if_pushredirected(
        &ctx,
        &repo,
        &live_commit_sync_config,
        &configs.repos,
    )
    .await?;
    if let Some(large_repo_config) = maybe_large_repo_config {
        let (large_repo, large_repo_import_setting, _syncers) = get_pushredirected_vars(
            &ctx,
            &repo,
            &repo_import_setting,
            &large_repo_config,
            &matches,
            live_commit_sync_config,
        )
        .await?;
        info!(
            ctx.logger(),
            "The repo we import {} into pushredirects to another repo {}. \
            The importing bookmark of the pushredirected repo is {} and \
            the destination bookmark is {}. If they don't exist already,
            make sure to notify Phabricator oncall to track these bookmarks as well!",
            repo.name(),
            large_repo.name(),
            large_repo_import_setting.importing_bookmark,
            large_repo_import_setting.dest_bookmark,
        );

        let large_repo_call_sign = large_repo_config.phabricator_callsign;
        if !phab_check_disabled && large_repo_call_sign.is_none() {
            return Err(format_err!(
                "Repo ({}) we push-redirect to doesn't have a callsign for checking the commits on Phabricator. \
                         Make sure the callsign for the repo is set in configerator: \
                         e.g CF/../source/scm/mononoke/repos/repos/hg.cinc",
                large_repo.name()
            ));
        }
    } else {
        info!(ctx.logger(), "There is no additional setup step needed!");
    }
    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = setup_app();
    let matches = app.get_matches();

    args::init_cachelib(fb, &matches, None);
    let logger = args::init_logging(fb, &matches);
    args::init_config_store(fb, &logger, &matches)?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::create_repo(fb, &logger, &matches);

    block_execute(
        async {
            let repo = repo.compat().await?;
            let mut recovery_fields = match matches.subcommand() {
                (CHECK_ADDITIONAL_SETUP_STEPS, Some(sub_arg_matches)) => {
                    check_additional_setup_steps(ctx, repo, fb, &sub_arg_matches, &matches).await?;
                    return Ok(());
                }
                (RECOVER_PROCESS, Some(sub_arg_matches)) => {
                    let saved_recovery_file_paths =
                        sub_arg_matches.value_of(SAVED_RECOVERY_FILE_PATH).unwrap();
                    fetch_recovery_state(&ctx, &saved_recovery_file_paths).await?
                }
                (IMPORT, Some(sub_arg_matches)) => setup_import_args(&sub_arg_matches)?,
                _ => return Err(format_err!("Invalid subcommand")),
            };

            match repo_import(ctx, repo, &mut recovery_fields, fb, &matches).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    save_importing_state(&recovery_fields).await?;
                    Err(e)
                }
            }
        },
        fb,
        "repo_import",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
