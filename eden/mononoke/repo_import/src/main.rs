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
use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
use borrowed::borrowed;
use cached_config::ConfigStore;
use cmdlib::args;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use cross_repo_sync::{create_commit_syncers, rewrite_commit, CommitSyncer};
use derived_data_utils::derived_data_utils;
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use import_tools::{GitimportPreferences, GitimportTarget};
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use manifest::ManifestOps;
use maplit::hashset;
use mercurial_types::{HgChangesetId, MPath};
use metaconfig_types::RepoConfig;
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime};
use movers::{DefaultAction, Mover};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use pushrebase::do_pushrebase_bonsai;
use serde::{Deserialize, Serialize};
use serde_json;
use slog::info;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};
use tokio::{process, time};
use topo_sort::sort_topological;
use unbundle::get_pushrebase_hooks;

mod cli;
mod tests;

use crate::cli::{
    setup_app, ARG_BATCH_SIZE, ARG_BOOKMARK_SUFFIX, ARG_COMMIT_AUTHOR, ARG_COMMIT_DATE_RFC3339,
    ARG_COMMIT_MESSAGE, ARG_DEST_BOOKMARK, ARG_DEST_PATH, ARG_GIT_REPOSITORY_PATH,
    ARG_HG_SYNC_CHECK_DISABLED, ARG_PHAB_CHECK_DISABLED, ARG_SLEEP_TIME, ARG_X_REPO_CHECK_DISABLED,
};

const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";

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
    call_sign: Option<String>,
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
}

async fn rewrite_file_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mover: &Mover,
    bonsai_values: &mut Vec<(ChangesetId, BonsaiChangeset)>,
) -> Result<Vec<ChangesetId>, Error> {
    let mut remapped_parents: HashMap<ChangesetId, ChangesetId> = HashMap::new();
    let mut index = 1;
    let map_size = bonsai_values.len();
    let mut bonsai_changesets = vec![];
    for (bcs_id, bcs) in bonsai_values {
        let bcs_mut = bcs.clone().into_mut();
        let rewritten_bcs_opt = rewrite_commit(
            ctx.clone(),
            bcs_mut,
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
                "Commit {}/{}: Remapped {:?} => {:?}", index, map_size, bcs_id, rewritten_bcs_id,
            );
            bonsai_changesets.push(rewritten_bcs);
        }
        index += 1;
    }

    bonsai_changesets = sort_bcs(&bonsai_changesets)?;
    let bcs_ids = get_cs_ids(&bonsai_changesets);
    info!(ctx.logger(), "Saving bonsai changesets");
    save_bonsai_changesets(bonsai_changesets.clone(), ctx.clone(), repo.clone())
        .compat()
        .await?;
    info!(ctx.logger(), "Saved bonsai changesets");
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
        let maybe_synced_cs_id: Option<ChangesetId> = large_to_small_syncer
            .sync_commit(&ctx, bcs_id.clone())
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
                    .compat()
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
    batch_size: usize,
    bookmark: &BookmarkName,
    checker_flags: &CheckerFlags,
    sleep_time: u64,
    mutable_counters: &SqlMutableCounters,
    maybe_small_repo_back_sync_vars: &Option<SmallRepoBackSyncVars>,
) -> Result<(), Error> {
    info!(ctx.logger(), "Start moving the bookmark");
    if shifted_bcs_ids.is_empty() {
        return Err(format_err!("There is no bonsai changeset present"));
    }

    let mut old_csid = match shifted_bcs_ids.first() {
        Some(first) => first,
        None => {
            return Err(format_err!("There is no bonsai changeset present"));
        }
    };
    let mut transaction = repo.update_bookmark_transaction(ctx.clone());
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
    for chunk in shifted_bcs_ids.chunks(batch_size) {
        transaction = repo.update_bookmark_transaction(ctx.clone());
        let curr_csid = match chunk.last() {
            Some(bcs_id) => bcs_id,
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

    if intersection.len() > 0 {
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
        .compat()
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
) -> Result<(), Error> {
    // if a check is disabled, we have already passed the check
    let mut passed_phab_check = checker_flags.phab_check_disabled;
    let mut _passed_x_repo_check = checker_flags.x_repo_check_disabled;
    let mut passed_hg_sync_check = checker_flags.hg_sync_check_disabled;

    let repo_id = repo.get_repoid();
    while !passed_phab_check {
        let call_sign = checker_flags.call_sign.as_ref().unwrap();
        passed_phab_check = phabricator_commit_check(&call_sign, &hg_csid).await?;
        if !passed_phab_check {
            info!(
                ctx.logger(),
                "Phabricator hasn't parsed commit: {:?}", hg_csid
            );
            time::delay_for(time::Duration::from_secs(sleep_time)).await;
        }
    }

    let largest_id = match repo
        .attribute_expected::<dyn BookmarkUpdateLog>()
        .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
        .await?
    {
        Some(id) => id,
        None => return Err(format_err!("Couldn't fetch id from bookmarks update log")),
    };

    /*
        In mutable counters table we store the latest bookmark id replayed by mercurial with
        LATEST_REPLAYED_REQUEST_KEY key. We use this key to extract the latest replayed id
        and compare it with the largest bookmark log id after we move the bookmark.
        If the replayed id is larger or equal to the bookmark id, we can try to move the bookmark
        to the next batch of commits
    */
    while !passed_hg_sync_check {
        let mut_counters_value = match mutable_counters
            .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
            .compat()
            .await?
        {
            Some(value) => value,
            None => {
                return Err(format_err!(
                    "Couldn't fetch the counter value from mutable_counters for repo_id {:?}",
                    repo_id
                ));
            }
        };
        passed_hg_sync_check = largest_id <= mut_counters_value.try_into().unwrap();
        if !passed_hg_sync_check {
            info!(
                ctx.logger(),
                "Waiting for {} to be replayed to hg, the latest replayed is {}",
                largest_id,
                mut_counters_value
            );
            time::delay_for(time::Duration::from_secs(sleep_time)).await;
        }
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

// Note: pushredirection only works from small repo to large repo.
async fn get_large_repo_config_if_pushredirected<'a>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    maybe_config_store: &Option<ConfigStore>,
    repos: &HashMap<String, RepoConfig>,
) -> Result<Option<RepoConfig>, Error> {
    let repo_id = repo.get_repoid();
    let config_store = maybe_config_store
        .as_ref()
        .ok_or(format_err!("failed to instantiate ConfigStore"))?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;
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
            .rename_bookmark(&importing_bookmark)
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
        .rename_bookmark(&dest_bookmark)
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

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = setup_app();
    let matches = app.get_matches();

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());
    let prefix = matches.value_of(ARG_DEST_PATH).unwrap();
    let dest_path_prefix = MPath::new(prefix)?;
    let bookmark_suffix = matches.value_of(ARG_BOOKMARK_SUFFIX).unwrap();
    let importing_bookmark = BookmarkName::new(format!("repo_import_{}", bookmark_suffix))?;
    let batch_size = matches.value_of(ARG_BATCH_SIZE).unwrap();
    let batch_size = batch_size.parse::<NonZeroUsize>()?.get();
    if !is_valid_bookmark_suffix(&bookmark_suffix) {
        return Err(format_err!(
            "The bookmark suffix contains invalid character(s).
            You can only use alphanumeric and \"./-_\" characters"
        ));
    }

    let phab_check_disabled = matches.is_present(ARG_PHAB_CHECK_DISABLED);
    let x_repo_check_disabled = matches.is_present(ARG_X_REPO_CHECK_DISABLED);
    let hg_sync_check_disabled = matches.is_present(ARG_HG_SYNC_CHECK_DISABLED);
    let sleep_time = matches.value_of(ARG_SLEEP_TIME).unwrap();
    let sleep_time = sleep_time.parse::<u64>()?;
    let dest_bookmark_name = matches.value_of(ARG_DEST_BOOKMARK).unwrap();
    let dest_bookmark = BookmarkName::new(dest_bookmark_name)?;
    let commit_author = matches.value_of(ARG_COMMIT_AUTHOR).unwrap();
    let commit_message = matches.value_of(ARG_COMMIT_MESSAGE).unwrap();
    let datetime = match matches.value_of(ARG_COMMIT_DATE_RFC3339) {
        Some(date) => DateTime::from_rfc3339(date)?,
        None => DateTime::now(),
    };
    let changeset_args = ChangesetArgs {
        author: commit_author.to_string(),
        message: commit_message.to_string(),
        datetime,
    };
    let mut repo_import_setting = RepoImportSetting {
        importing_bookmark,
        dest_bookmark,
    };
    args::init_cachelib(fb, &matches, None);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::create_repo(fb, &logger, &matches);
    block_execute(
        async {
            let mut repo = repo.compat().await?;
            let (_, mut repo_config) =
                args::get_config_by_repoid(ctx.fb, &matches, repo.get_repoid())?;
            let call_sign = repo_config.phabricator_callsign.clone();
            if !phab_check_disabled && call_sign.is_none() {
                return Err(format_err!(
                    "The repo we import to doesn't have a callsign. \
                     Make sure the callsign for the repo is set in configerator: \
                     e.g CF/../source/scm/mononoke/repos/repos/hg.cinc"
                ));
            }
            let checker_flags = CheckerFlags {
                phab_check_disabled,
                x_repo_check_disabled,
                hg_sync_check_disabled,
                call_sign,
            };
            let maybe_config_store = args::maybe_init_config_store(fb, &logger, &matches);
            let configs = args::load_repo_configs(fb, &matches)?;
            let mysql_options = args::parse_mysql_options(&matches);
            let readonly_storage = args::parse_readonly_storage(&matches);

            let maybe_large_repo_config = get_large_repo_config_if_pushredirected(
                &ctx,
                &repo,
                &maybe_config_store,
                &configs.repos,
            )
            .await?;
            let mut maybe_small_repo_back_sync_vars = None;
            let mut movers = vec![movers::mover_factory(
                HashMap::new(),
                DefaultAction::PrependPrefix(dest_path_prefix),
            )?];

            if let Some(large_repo_config) = maybe_large_repo_config {
                let large_repo_id = large_repo_config.repoid;
                let large_repo = args::open_repo_with_repo_id(fb, &logger, large_repo_id, &matches)
                    .compat()
                    .await?;
                let commit_sync_config = large_repo_config.commit_sync_config.clone().unwrap();
                if commit_sync_config.small_repos.len() > 1 {
                    return Err(format_err!(
                        "Currently repo_import tool doesn't support backsyncing into multiple small repos for large repo {:?}, name: {}",
                        large_repo_id,
                        large_repo.name()
                    ));
                }
                let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(fb, &matches)
                    .compat()
                    .await?;
                let syncers = create_commit_syncers(
                    repo.clone(),
                    large_repo.clone(),
                    &commit_sync_config,
                    mapping.clone(),
                )?;
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
                });

                movers.push(syncers.small_to_large.get_mover().clone());
                repo_import_setting =
                    get_large_repo_setting(&ctx, &repo_import_setting, &syncers.small_to_large)
                        .await?;
                repo = large_repo;
                repo_config = large_repo_config;
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
            let prefs = GitimportPreferences::default();
            let target = GitimportTarget::FullRepo;
            info!(ctx.logger(), "Started importing git commits to Mononoke");
            let import_map = import_tools::gitimport(&ctx, &repo, &path, target, prefs).await?;
            info!(ctx.logger(), "Added commits to Mononoke");
            let mut bonsai_values: Vec<(ChangesetId, BonsaiChangeset)> =
                import_map.values().cloned().collect();

            let shifted_bcs_ids =
                rewrite_file_paths(&ctx, &repo, &combined_mover, &mut bonsai_values).await?;

            let derive_changesets = derive_bonsais_single_repo(&ctx, &repo, &shifted_bcs_ids);

            let backsync_and_derive_changesets = {
                borrowed!(ctx, shifted_bcs_ids);

                async move {
                    let vars = match maybe_small_repo_back_sync_vars {
                        Some(vars) => {
                            info!(ctx.logger(), "Backsyncing changesets");
                            vars
                        }
                        None => return Ok(None),
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
                    Ok(Some(vars))
                }
            };

            info!(ctx.logger(), "Start deriving data types");
            let ((), maybe_small_repo_back_sync_vars) =
                future::try_join(derive_changesets, backsync_and_derive_changesets).await?;
            info!(ctx.logger(), "Finished deriving data types");

            move_bookmark(
                &ctx,
                &repo,
                &shifted_bcs_ids,
                batch_size,
                &repo_import_setting.importing_bookmark,
                &checker_flags,
                sleep_time,
                &mutable_counters,
                &maybe_small_repo_back_sync_vars,
            )
            .await?;

            let imported_cs_id = match shifted_bcs_ids.last() {
                Some(bcs_id) => bcs_id,
                None => return Err(format_err!("There is no bonsai changeset present")),
            };

            let merged_cs_id = merge_imported_commit(
                &ctx,
                &repo,
                imported_cs_id.clone(),
                &repo_import_setting.dest_bookmark,
                changeset_args,
            )
            .await?;
            push_merge_commit(
                &ctx,
                &repo,
                merged_cs_id,
                &repo_import_setting.dest_bookmark,
                &repo_config,
            )
            .await?;
            Ok(())
        },
        fb,
        "repo_import",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
