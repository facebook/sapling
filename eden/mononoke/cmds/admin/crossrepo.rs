/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, format_err, Context, Error};
use backsyncer::format_counter as format_backsyncer_counter;
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bookmark_renaming::get_small_to_large_renamer;
use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use cross_repo_sync::{
    types::{Large, Small},
    validation::{self, BookmarkDiff},
    CommitSyncContext, CommitSyncRepos, CommitSyncer,
};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, try_join};
use futures_ext::FutureExt;
use itertools::Itertools;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use maplit::{btreemap, hashmap};
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::{CommitSyncConfig, RepoConfig};
use mononoke_types::{BonsaiChangesetMut, ChangesetId, DateTime, RepositoryId};
use mutable_counters::MutableCounters;
use mutable_counters::SqlMutableCounters;
use pushrebase::FAILUPUSHREBASE_EXTRA;
use slog::{info, warn, Logger};
use std::convert::TryInto;
use std::sync::Arc;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::common::get_source_target_repos_and_mapping;
use crate::error::SubcommandError;

pub const CROSSREPO: &str = "crossrepo";
const AUTHOR_ARG: &str = "author";
const MAP_SUBCOMMAND: &str = "map";
const PREPARE_ROLLOUT_SUBCOMMAND: &str = "prepare-rollout";
const PUSHREDIRECTION_SUBCOMMAND: &str = "pushredirection";
const VERIFY_WC_SUBCOMMAND: &str = "verify-wc";
const VERIFY_BOOKMARKS_SUBCOMMAND: &str = "verify-bookmarks";
const HASH_ARG: &str = "HASH";
const LARGE_REPO_HASH_ARG: &str = "LARGE_REPO_HASH";
const UPDATE_LARGE_REPO_BOOKMARKS: &str = "update-large-repo-bookmarks";
const LARGE_REPO_BOOKMARK_ARG: &str = "large-repo-bookmark";
const MAPPING_VERSION_ARG: &str = "mapping-version";
const CHANGE_MAPPING_VERSION_SUBCOMMAND: &str = "change-mapping-version";

const SUBCOMMAND_CONFIG: &str = "config";
const SUBCOMMAND_BY_VERSION: &str = "by-version";
const SUBCOMMAND_LIST: &str = "list";
const SUBCOMMAND_CURRENT: &str = "current";
const ARG_VERSION_NAME: &str = "version-name";
const ARG_WITH_CONTENTS: &str = "with-contents";

pub async fn subcommand_crossrepo<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let config_store = args::init_config_store(fb, &logger, &matches)?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(&logger, &config_store)?;

    args::init_cachelib(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    match sub_m.subcommand() {
        (MAP_SUBCOMMAND, Some(sub_sub_m)) => {
            let (source_repo, target_repo, mapping) =
                get_source_target_repos_and_mapping(fb, logger, matches).await?;

            let hash = sub_sub_m.value_of(HASH_ARG).unwrap().to_owned();
            subcommand_map(ctx, source_repo, target_repo, mapping, hash).await
        }
        (VERIFY_WC_SUBCOMMAND, Some(sub_sub_m)) => {
            let (source_repo, target_repo, mapping) =
                get_source_target_repos_and_mapping(fb, logger, matches).await?;

            let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> =
                Arc::new(live_commit_sync_config);
            let commit_syncer = get_large_to_small_commit_syncer(
                &ctx,
                source_repo,
                target_repo,
                live_commit_sync_config,
                mapping,
            )?;

            let large_hash = {
                let large_hash = sub_sub_m.value_of(LARGE_REPO_HASH_ARG).unwrap().to_owned();
                let large_repo = commit_syncer.get_large_repo();
                helpers::csid_resolve(ctx.clone(), large_repo.clone(), large_hash)
                    .boxify()
                    .compat()
                    .await?
            };

            validation::verify_working_copy(ctx.clone(), commit_syncer, large_hash)
                .await
                .map_err(|e| e.into())
        }
        (VERIFY_BOOKMARKS_SUBCOMMAND, Some(sub_sub_m)) => {
            let config_store = args::init_config_store(fb, ctx.logger(), matches)?;

            let (source_repo, target_repo, mapping) =
                get_source_target_repos_and_mapping(fb, logger, matches).await?;
            let source_repo_id = source_repo.get_repoid();

            let (_, source_repo_config) =
                args::get_config_by_repoid(config_store, matches, source_repo_id)?;

            let update_large_repo_bookmarks = sub_sub_m.is_present(UPDATE_LARGE_REPO_BOOKMARKS);

            subcommand_verify_bookmarks(
                ctx,
                source_repo,
                source_repo_config,
                target_repo,
                mapping,
                update_large_repo_bookmarks,
                Arc::new(live_commit_sync_config),
            )
            .await
        }
        (SUBCOMMAND_CONFIG, Some(sub_sub_m)) => {
            run_config_sub_subcommand(ctx, matches, sub_sub_m, live_commit_sync_config).await
        }
        (PUSHREDIRECTION_SUBCOMMAND, Some(sub_sub_m)) => {
            run_pushredirection_subcommand(fb, ctx, matches, sub_sub_m, live_commit_sync_config)
                .await
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn run_config_sub_subcommand<'a>(
    ctx: CoreContext,
    matches: &'a MononokeMatches<'_>,
    config_subcommand_matches: &'a ArgMatches<'a>,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(), SubcommandError> {
    let config_store = args::init_config_store(ctx.fb, ctx.logger(), matches)?;

    let repo_id = args::get_repo_id(config_store, matches)?;

    match config_subcommand_matches.subcommand() {
        (SUBCOMMAND_BY_VERSION, Some(sub_m)) => {
            let version_name: String = sub_m.value_of(ARG_VERSION_NAME).unwrap().to_string();
            subcommand_by_version(repo_id, live_commit_sync_config, version_name)
                .await
                .map_err(|e| e.into())
        }
        (SUBCOMMAND_CURRENT, Some(sub_m)) => {
            let with_contents = sub_m.is_present(ARG_WITH_CONTENTS);
            subcommand_current(ctx, repo_id, live_commit_sync_config, with_contents)
                .await
                .map_err(|e| e.into())
        }
        (SUBCOMMAND_LIST, Some(sub_m)) => {
            let with_contents = sub_m.is_present(ARG_WITH_CONTENTS);
            subcommand_list(repo_id, live_commit_sync_config, with_contents)
                .await
                .map_err(|e| e.into())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn run_pushredirection_subcommand<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    matches: &'a MononokeMatches<'_>,
    config_subcommand_matches: &'a ArgMatches<'a>,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(), SubcommandError> {
    let config_store = args::init_config_store(fb, ctx.logger(), matches)?;

    let (source_repo, target_repo, mapping) =
        get_source_target_repos_and_mapping(fb, ctx.logger().clone(), matches).await?;

    let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> = Arc::new(live_commit_sync_config);

    match config_subcommand_matches.subcommand() {
        (PREPARE_ROLLOUT_SUBCOMMAND, Some(_sub_m)) => {
            let commit_syncer = get_large_to_small_commit_syncer(
                &ctx,
                source_repo,
                target_repo,
                live_commit_sync_config.clone(),
                mapping,
            )?;

            if live_commit_sync_config
                .push_redirector_enabled_for_public(commit_syncer.get_small_repo().get_repoid())
            {
                return Err(format_err!(
                    "not allowed to run {} if pushredirection is enabled",
                    PREPARE_ROLLOUT_SUBCOMMAND
                )
                .into());
            }

            let small_repo = commit_syncer.get_small_repo();
            let large_repo = commit_syncer.get_large_repo();
            let largest_id = large_repo
                .attribute_expected::<dyn BookmarkUpdateLog>()
                .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
                .await?
                .ok_or_else(|| anyhow!("No bookmarks update log entries for large repo"))?;

            let mutable_counters =
                args::open_source_sql::<SqlMutableCounters>(fb, config_store, &matches)
                    .await
                    .context("While opening SqlMutableCounters")?;

            let counter = format_backsyncer_counter(&large_repo.get_repoid());
            info!(
                ctx.logger(),
                "setting value {} to counter {} for repo {}",
                largest_id,
                counter,
                small_repo.get_repoid()
            );
            let res = mutable_counters
                .set_counter(
                    ctx.clone(),
                    small_repo.get_repoid(),
                    &counter,
                    largest_id.try_into().unwrap(),
                    None, // prev_value
                )
                .compat()
                .await?;

            if !res {
                return Err(anyhow!("failed to set backsyncer counter").into());
            }
            info!(ctx.logger(), "successfully updated the counter");

            Ok(())
        }
        (CHANGE_MAPPING_VERSION_SUBCOMMAND, Some(sub_m)) => {
            let commit_syncer = get_large_to_small_commit_syncer(
                &ctx,
                source_repo,
                target_repo,
                live_commit_sync_config.clone(),
                mapping,
            )?;

            if live_commit_sync_config
                .push_redirector_enabled_for_public(commit_syncer.get_small_repo().get_repoid())
            {
                return Err(format_err!(
                    "not allowed to run {} if pushredirection is enabled",
                    CHANGE_MAPPING_VERSION_SUBCOMMAND
                )
                .into());
            }

            let large_bookmark = Large(
                sub_m
                    .value_of(LARGE_REPO_BOOKMARK_ARG)
                    .map(BookmarkName::new)
                    .transpose()?
                    .ok_or_else(|| format_err!("{} is not specified", LARGE_REPO_BOOKMARK_ARG))?,
            );
            let small_bookmark = Small(
                commit_syncer.get_bookmark_renamer(&ctx)?(&large_bookmark).ok_or_else(|| {
                    format_err!("{} bookmark doesn't remap to small repo", large_bookmark)
                })?,
            );

            let large_repo = Large(commit_syncer.get_large_repo());
            let small_repo = Small(commit_syncer.get_small_repo());
            let large_bookmark_value =
                Large(get_bookmark_value(&ctx, &large_repo, &large_bookmark).await?);
            let small_bookmark_value =
                Small(get_bookmark_value(&ctx, &small_repo, &small_bookmark).await?);

            let mapping_version = sub_m
                .value_of(MAPPING_VERSION_ARG)
                .ok_or_else(|| format_err!("{} is not specified", MAPPING_VERSION_ARG))?;
            let mapping_version = CommitSyncConfigVersion(mapping_version.to_string());
            if !commit_syncer.version_exists(&mapping_version)? {
                return Err(format_err!("{} version does not exist", mapping_version).into());
            }

            let large_cs_id = create_empty_commit_for_mapping_change(
                &ctx,
                sub_m,
                &large_repo,
                &small_repo,
                &large_bookmark_value,
                &mapping_version,
            )
            .await?;

            let maybe_rewritten_small_cs_id = commit_syncer
                .unsafe_always_rewrite_sync_commit(
                    &ctx,
                    large_cs_id.0,
                    Some(hashmap! {
                      large_bookmark_value.0.clone() => small_bookmark_value.0.clone(),
                    }),
                    &mapping_version,
                    CommitSyncContext::AdminChangeMapping,
                )
                .await?;

            let rewritten_small_cs_id = Small(maybe_rewritten_small_cs_id.ok_or_else(|| {
                format_err!("{} was rewritten into non-existent commit", large_cs_id)
            })?);

            let f1 = move_bookmark(
                &ctx,
                &large_repo,
                &large_bookmark,
                *large_bookmark_value,
                *large_cs_id,
            );

            let f2 = move_bookmark(
                &ctx,
                &small_repo,
                &small_bookmark,
                *small_bookmark_value,
                *rewritten_small_cs_id,
            );

            try_join!(f1, f2)?;

            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn create_empty_commit_for_mapping_change(
    ctx: &CoreContext,
    sub_m: &ArgMatches<'_>,
    large_repo: &Large<&BlobRepo>,
    small_repo: &Small<&BlobRepo>,
    parent: &Large<ChangesetId>,
    mapping_version: &CommitSyncConfigVersion,
) -> Result<Large<ChangesetId>, Error> {
    let author = sub_m
        .value_of(AUTHOR_ARG)
        .ok_or_else(|| format_err!("{} is not specified", AUTHOR_ARG))?;

    let commit_msg = format!(
        "Changing synced mapping version to {} for {}->{} sync",
        mapping_version,
        large_repo.name(),
        small_repo.name(),
    );
    // Create an empty commit on top of large bookmark
    let bcs = BonsaiChangesetMut {
        parents: vec![parent.0.clone()],
        author: author.to_string(),
        author_date: DateTime::now(),
        committer: None,
        committer_date: None,
        message: commit_msg,
        extra: btreemap! {
            FAILUPUSHREBASE_EXTRA.to_string() => b"1".to_vec(),
        },
        file_changes: btreemap! {},
    }
    .freeze()?;

    let large_cs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), large_repo.0.clone()).await?;

    Ok(Large(large_cs_id))
}

async fn get_bookmark_value(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: &BookmarkName,
) -> Result<ChangesetId, Error> {
    let maybe_bookmark_value = repo.get_bonsai_bookmark(ctx.clone(), &bookmark).await?;

    maybe_bookmark_value.ok_or_else(|| format_err!("{} is not found in {}", bookmark, repo.name()))
}

async fn move_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: &BookmarkName,
    prev_value: ChangesetId,
    new_value: ChangesetId,
) -> Result<(), Error> {
    let mut book_txn = repo.update_bookmark_transaction(ctx.clone());

    info!(
        ctx.logger(),
        "moving {} to {} in {}",
        bookmark,
        new_value,
        repo.name()
    );
    book_txn.update(
        &bookmark,
        new_value,
        prev_value,
        BookmarkUpdateReason::ManualMove,
        None,
    )?;

    let res = book_txn.commit().await?;

    if res {
        Ok(())
    } else {
        Err(format_err!(
            "failed to move bookmark {} in {}",
            bookmark,
            repo.name()
        ))
    }
}

fn print_commit_sync_config(csc: CommitSyncConfig, line_prefix: &str) {
    println!("{}large repo: {}", line_prefix, csc.large_repo_id);
    println!(
        "{}common pushrebase bookmarks: {:?}",
        line_prefix, csc.common_pushrebase_bookmarks
    );
    println!("{}version name: {}", line_prefix, csc.version_name);
    for (small_repo_id, small_repo_config) in csc
        .small_repos
        .into_iter()
        .sorted_by_key(|(small_repo_id, _)| *small_repo_id)
    {
        println!("{}  small repo: {}", line_prefix, small_repo_id);
        println!(
            "{}  bookmark prefix: {}",
            line_prefix, small_repo_config.bookmark_prefix
        );
        println!(
            "{}  direction: {:?}",
            line_prefix, small_repo_config.direction
        );
        println!(
            "{}  default action: {:?}",
            line_prefix, small_repo_config.default_action
        );
        println!("{}  prefix map:", line_prefix);
        for (from, to) in small_repo_config
            .map
            .into_iter()
            .sorted_by_key(|(from, _)| from.clone())
        {
            println!("{}    {}->{}", line_prefix, from, to);
        }
    }
}

async fn subcommand_current<'a, L: LiveCommitSyncConfig>(
    ctx: CoreContext,
    repo_id: RepositoryId,
    live_commit_sync_config: L,
    with_contents: bool,
) -> Result<(), Error> {
    let csc = live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_id)?;
    if with_contents {
        print_commit_sync_config(csc, "");
    } else {
        println!("{}", csc.version_name);
    }

    Ok(())
}

async fn subcommand_list<'a, L: LiveCommitSyncConfig>(
    repo_id: RepositoryId,
    live_commit_sync_config: L,
    with_contents: bool,
) -> Result<(), Error> {
    let all = live_commit_sync_config.get_all_commit_sync_config_versions(repo_id)?;
    for (version_name, csc) in all.into_iter().sorted_by_key(|(vn, _)| vn.clone()) {
        if with_contents {
            println!("{}:", version_name);
            print_commit_sync_config(csc, "  ");
            println!("\n");
        } else {
            println!("{}", version_name);
        }
    }

    Ok(())
}

async fn subcommand_by_version<'a, L: LiveCommitSyncConfig>(
    repo_id: RepositoryId,
    live_commit_sync_config: L,
    version_name: String,
) -> Result<(), Error> {
    let csc = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_id, &CommitSyncConfigVersion(version_name))?;
    print_commit_sync_config(csc, "");
    Ok(())
}

async fn subcommand_map(
    ctx: CoreContext,
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
    hash: String,
) -> Result<(), SubcommandError> {
    let source_repo_id = source_repo.get_repoid();
    let target_repo_id = target_repo.get_repoid();
    let source_hash = helpers::csid_resolve(ctx.clone(), source_repo, hash)
        .compat()
        .await?;

    let mappings = mapping
        .get(ctx.clone(), source_repo_id, source_hash, target_repo_id)
        .compat()
        .await?;

    if mappings.is_empty() {
        let exists = target_repo
            .changeset_exists_by_bonsai(ctx, source_hash.clone())
            .await?;

        if exists {
            println!(
                "Hash {} not currently remapped (but present in target as-is)",
                source_hash
            );
        } else {
            println!("Hash {} not currently remapped", source_hash);
        }
    } else {
        for (target_hash, maybe_version_name) in mappings {
            match maybe_version_name {
                Some(version_name) => {
                    println!(
                        "Hash {} maps to {}, used {:?}",
                        source_hash, target_hash, version_name
                    );
                }
                None => {
                    println!("Hash {} maps to {}", source_hash, target_hash);
                }
            }
        }
    }

    Ok(())
}

async fn subcommand_verify_bookmarks(
    ctx: CoreContext,
    source_repo: BlobRepo,
    source_repo_config: RepoConfig,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
    should_update_large_repo_bookmarks: bool,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), SubcommandError> {
    let commit_syncer = get_large_to_small_commit_syncer(
        &ctx,
        source_repo,
        target_repo,
        live_commit_sync_config,
        mapping.clone(),
    )?;

    let large_repo = commit_syncer.get_large_repo();
    let small_repo = commit_syncer.get_small_repo();
    let diff = validation::find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

    if diff.is_empty() {
        info!(ctx.logger(), "all is well!");
        Ok(())
    } else {
        if should_update_large_repo_bookmarks {
            let commit_sync_config = source_repo_config
                .commit_sync_config
                .as_ref()
                .ok_or_else(|| format_err!("missing CommitSyncMapping config"))?;
            update_large_repo_bookmarks(
                ctx.clone(),
                &diff,
                small_repo,
                commit_sync_config,
                large_repo,
                mapping,
            )
            .await?;

            Ok(())
        } else {
            let source_repo = commit_syncer.get_source_repo();
            let target_repo = commit_syncer.get_target_repo();
            for d in &diff {
                use BookmarkDiff::*;
                match d {
                    InconsistentValue {
                        target_bookmark,
                        target_cs_id,
                        source_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "inconsistent value of {}: '{}' has {}, but '{}' bookmark points to {:?}",
                            target_bookmark,
                            target_repo.name(),
                            target_cs_id,
                            source_repo.name(),
                            source_cs_id,
                        );
                    }
                    MissingInTarget {
                        target_bookmark,
                        source_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "'{}' doesn't have bookmark {} but '{}' has it and it points to {}",
                            target_repo.name(),
                            target_bookmark,
                            source_repo.name(),
                            source_cs_id,
                        );
                    }
                    NoSyncOutcome { target_bookmark } => {
                        warn!(
                            ctx.logger(),
                            "'{}' has a bookmark {} but it points to a commit that has no \
                             equivalent in '{}'. If it's a shared bookmark (e.g. master) \
                             that might mean that it points to a commit from another repository",
                            target_repo.name(),
                            target_bookmark,
                            source_repo.name(),
                        );
                    }
                }
            }
            Err(format_err!("found {} inconsistencies", diff.len()).into())
        }
    }
}

async fn update_large_repo_bookmarks(
    ctx: CoreContext,
    diff: &Vec<BookmarkDiff>,
    small_repo: &BlobRepo,
    commit_sync_config: &CommitSyncConfig,
    large_repo: &BlobRepo,
    mapping: SqlSyncedCommitMapping,
) -> Result<(), Error> {
    warn!(
        ctx.logger(),
        "found {} inconsistencies, trying to update them...",
        diff.len()
    );
    let mut book_txn = large_repo.update_bookmark_transaction(ctx.clone());

    let bookmark_renamer = get_small_to_large_renamer(commit_sync_config, small_repo.get_repoid())?;
    for d in diff {
        if commit_sync_config
            .common_pushrebase_bookmarks
            .contains(d.target_bookmark())
        {
            info!(
                ctx.logger(),
                "skipping {} because it's a common bookmark",
                d.target_bookmark()
            );
            continue;
        }

        use validation::BookmarkDiff::*;
        match d {
            InconsistentValue {
                target_bookmark,
                target_cs_id,
                ..
            } => {
                let large_cs_ids = mapping
                    .get(
                        ctx.clone(),
                        small_repo.get_repoid(),
                        *target_cs_id,
                        large_repo.get_repoid(),
                    )
                    .compat()
                    .await?;

                if large_cs_ids.len() > 1 {
                    return Err(format_err!(
                        "multiple remappings of {} in {}: {:?}",
                        *target_cs_id,
                        large_repo.get_repoid(),
                        large_cs_ids
                    ));
                } else if let Some((large_cs_id, _)) = large_cs_ids.into_iter().next() {
                    let reason = BookmarkUpdateReason::XRepoSync;
                    let large_bookmark = bookmark_renamer(&target_bookmark).ok_or(format_err!(
                        "small bookmark {} remaps to nothing",
                        target_bookmark
                    ))?;

                    info!(ctx.logger(), "setting {} {}", large_bookmark, large_cs_id);
                    book_txn.force_set(&large_bookmark, large_cs_id, reason, None)?;
                } else {
                    warn!(
                        ctx.logger(),
                        "{} from small repo doesn't remap to large repo", target_cs_id,
                    );
                }
            }
            MissingInTarget {
                target_bookmark, ..
            } => {
                warn!(
                    ctx.logger(),
                    "large repo bookmark (renames to {}) not found in small repo", target_bookmark,
                );
                let large_bookmark = bookmark_renamer(target_bookmark).ok_or(format_err!(
                    "small bookmark {} remaps to nothing",
                    target_bookmark
                ))?;
                let reason = BookmarkUpdateReason::XRepoSync;
                info!(ctx.logger(), "deleting {}", large_bookmark);
                book_txn.force_delete(&large_bookmark, reason, None)?;
            }
            NoSyncOutcome { target_bookmark } => {
                warn!(
                    ctx.logger(),
                    "Not updating {} because it points to a commit that has no \
                     equivalent in source repo.",
                    target_bookmark,
                );
            }
        }
    }

    book_txn.commit().await?;
    Ok(())
}

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let map_subcommand = SubCommand::with_name(MAP_SUBCOMMAND)
        .about("Check cross-repo commit mapping")
        .arg(
            Arg::with_name(HASH_ARG)
                .required(true)
                .help("bonsai changeset hash to map"),
        );

    let verify_wc_subcommand = SubCommand::with_name(VERIFY_WC_SUBCOMMAND)
        .about("verify working copy")
        .arg(
            Arg::with_name(LARGE_REPO_HASH_ARG)
                .required(true)
                .help("bonsai changeset hash from large repo to verify"),
        );

    let verify_bookmarks_subcommand = SubCommand::with_name(VERIFY_BOOKMARKS_SUBCOMMAND).about(
        "verify that bookmarks are the same in small and large repo (subject to bookmark renames)",
    ).arg(
        Arg::with_name(UPDATE_LARGE_REPO_BOOKMARKS)
            .long(UPDATE_LARGE_REPO_BOOKMARKS)
            .required(false)
            .takes_value(false)
            .help("update any inconsistencies between bookmarks (except for the common bookmarks between large and small repo e.g. 'master')"),
    );

    let commit_sync_config_subcommand = {
        let by_version_subcommand = SubCommand::with_name(SUBCOMMAND_BY_VERSION)
            .about("print info about a particular version of CommitSyncConfig")
            .arg(
                Arg::with_name(ARG_VERSION_NAME)
                    .required(true)
                    .takes_value(true)
                    .help("commit sync config version name to query"),
            );

        let list_subcommand = SubCommand::with_name(SUBCOMMAND_LIST)
            .about("list all available CommitSyncConfig versions for repo")
            .arg(
                Arg::with_name(ARG_WITH_CONTENTS)
                    .long(ARG_WITH_CONTENTS)
                    .required(false)
                    .takes_value(false)
                    .help("Do not just print version names, also include config bodies"),
            );

        let current_subcommand = SubCommand::with_name(SUBCOMMAND_CURRENT)
            .about("print current CommitSyncConfig version for repo")
            .arg(
                Arg::with_name(ARG_WITH_CONTENTS)
                    .long(ARG_WITH_CONTENTS)
                    .required(false)
                    .takes_value(false)
                    .help("Do not just print version name, also include config body"),
            );

        SubCommand::with_name(SUBCOMMAND_CONFIG)
            .about("query available CommitSyncConfig versions for repo")
            .subcommand(current_subcommand)
            .subcommand(list_subcommand)
            .subcommand(by_version_subcommand)
    };

    let prepare_rollout_subcommand = SubCommand::with_name(PREPARE_ROLLOUT_SUBCOMMAND)
        .about("command to prepare rollout of pushredirection");

    let change_mapping_version = SubCommand::with_name(CHANGE_MAPPING_VERSION_SUBCOMMAND)
        .about(
            "a command to change mapping version for a given bookmark. \
        Note that this command doesn't check that the working copies of source and target repo \
        are equivalent according to the new mapping. This needs to ensured before calling this command",
        )
        .arg(
            Arg::with_name(AUTHOR_ARG)
                .long(AUTHOR_ARG)
                .required(true)
                .takes_value(true)
                .help("Author of the commit that will change the mapping"),
        )
        .arg(
            Arg::with_name(LARGE_REPO_BOOKMARK_ARG)
                .long(LARGE_REPO_BOOKMARK_ARG)
                .required(true)
                .takes_value(true)
                .help("bookmark in the large repo"),
        )
        .arg(
            Arg::with_name(MAPPING_VERSION_ARG)
                .long(MAPPING_VERSION_ARG)
                .required(true)
                .takes_value(true)
                .help("mapping version to change to"),
        );

    let pushredirection_subcommand = SubCommand::with_name(PUSHREDIRECTION_SUBCOMMAND)
        .about("helper commands to enable/disable pushredirection")
        .subcommand(prepare_rollout_subcommand)
        .subcommand(change_mapping_version);

    SubCommand::with_name(CROSSREPO)
        .subcommand(map_subcommand)
        .subcommand(verify_wc_subcommand)
        .subcommand(verify_bookmarks_subcommand)
        .subcommand(commit_sync_config_subcommand)
        .subcommand(pushredirection_subcommand)
}

fn get_large_to_small_commit_syncer(
    ctx: &CoreContext,
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    mapping: SqlSyncedCommitMapping,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let commit_sync_config =
        live_commit_sync_config.get_current_commit_sync_config(ctx, source_repo.get_repoid())?;

    let (large_repo, small_repo) = if commit_sync_config.large_repo_id == source_repo.get_repoid()
        && commit_sync_config
            .small_repos
            .contains_key(&target_repo.get_repoid())
    {
        (source_repo, target_repo)
    } else if commit_sync_config.large_repo_id == target_repo.get_repoid()
        && commit_sync_config
            .small_repos
            .contains_key(&source_repo.get_repoid())
    {
        (target_repo, source_repo)
    } else {
        return Err(format_err!(
            "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
            source_repo.get_repoid(),
            target_repo.get_repoid()
        ));
    };

    let commit_sync_repos = CommitSyncRepos::LargeToSmall {
        large_repo,
        small_repo,
    };

    Ok(CommitSyncer::new(
        &ctx,
        mapping,
        commit_sync_repos,
        live_commit_sync_config,
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use bookmarks::BookmarkName;
    use cross_repo_sync::{
        types::{Source, Target},
        validation::find_bookmark_diff,
        CommitSyncDataProvider, SyncData,
    };
    use fixtures::{linear, set_bookmark};
    use futures_old::stream::Stream;
    use maplit::{hashmap, hashset};
    use metaconfig_types::{
        CommitSyncConfig, CommitSyncConfigVersion, CommitSyncDirection,
        DefaultSmallToLargeCommitSyncPathAction, SmallRepoCommitSyncConfig,
    };
    use mononoke_types::{MPath, RepositoryId};
    use revset::AncestorsNodeStream;
    use sql_construct::SqlConstruct;
    use std::{collections::HashSet, sync::Arc};
    // To support async tests
    use synced_commit_mapping::SyncedCommitMappingEntry;

    fn noop_book_renamer(bookmark_name: &BookmarkName) -> Option<BookmarkName> {
        Some(bookmark_name.clone())
    }

    fn identity_mover(p: &MPath) -> Result<Option<MPath>, Error> {
        Ok(Some(p.clone()))
    }

    #[fbinit::test]
    fn test_bookmark_diff(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_bookmark_diff_impl(fb))
    }

    async fn test_bookmark_diff_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(fb, CommitSyncDirection::LargeToSmall).await?;

        let small_repo = commit_syncer.get_small_repo();
        let large_repo = commit_syncer.get_large_repo();

        let master = BookmarkName::new("master")?;
        let maybe_master_val = small_repo.get_bonsai_bookmark(ctx.clone(), &master).await?;
        let master_val = maybe_master_val.ok_or(Error::msg("master not found"))?;

        // Everything is identical - no diff at all
        {
            let diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

            assert!(diff.is_empty());
        }

        // Move bookmark to another changeset
        let another_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
        set_bookmark(fb, small_repo.clone(), another_hash, master.clone()).await;
        let another_bcs_id =
            helpers::csid_resolve(ctx.clone(), small_repo.clone(), another_hash.to_string())
                .compat()
                .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

        let mut expected_diff = hashset! {
            BookmarkDiff::InconsistentValue {
                target_bookmark: master.clone(),
                target_cs_id: another_bcs_id,
                source_cs_id: Some(master_val),
            }
        };
        assert!(!actual_diff.is_empty());
        assert_eq!(
            actual_diff.into_iter().collect::<HashSet<_>>(),
            expected_diff,
        );

        // Create another bookmark
        let another_book = BookmarkName::new("newbook")?;
        set_bookmark(fb, small_repo.clone(), another_hash, another_book.clone()).await;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

        expected_diff.insert(BookmarkDiff::InconsistentValue {
            target_bookmark: another_book,
            target_cs_id: another_bcs_id,
            source_cs_id: None,
        });
        assert_eq!(
            actual_diff.clone().into_iter().collect::<HashSet<_>>(),
            expected_diff
        );

        // Update the bookmarks
        {
            let small_repo_sync_config = SmallRepoCommitSyncConfig {
                default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                direction: CommitSyncDirection::SmallToLarge,
                map: Default::default(),
                bookmark_prefix: Default::default(),
            };
            let mut commit_sync_config = CommitSyncConfig {
                large_repo_id: large_repo.get_repoid(),
                common_pushrebase_bookmarks: vec![master.clone()],
                small_repos: hashmap! {
                    small_repo.get_repoid() => small_repo_sync_config,
                },
                version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            };
            update_large_repo_bookmarks(
                ctx.clone(),
                &actual_diff,
                small_repo,
                &commit_sync_config,
                large_repo,
                commit_syncer.get_mapping().clone(),
            )
            .await?;

            let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

            // Master bookmark hasn't been updated because it's a common pushrebase bookmark
            let expected_diff = hashset! {
                BookmarkDiff::InconsistentValue {
                    target_bookmark: master.clone(),
                    target_cs_id: another_bcs_id,
                    source_cs_id: Some(master_val),
                }
            };
            assert_eq!(
                actual_diff.clone().into_iter().collect::<HashSet<_>>(),
                expected_diff,
            );

            // Now remove master bookmark from common_pushrebase_bookmarks and update large repo
            // bookmarks again
            commit_sync_config.common_pushrebase_bookmarks = vec![];

            update_large_repo_bookmarks(
                ctx.clone(),
                &actual_diff,
                small_repo,
                &commit_sync_config,
                large_repo,
                commit_syncer.get_mapping().clone(),
            )
            .await?;
            let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;
            assert!(actual_diff.is_empty());
        }
        Ok(())
    }

    async fn init(
        fb: FacebookInit,
        direction: CommitSyncDirection,
    ) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
        let ctx = CoreContext::test_mock(fb);
        let small_repo = linear::getrepo_with_id(fb, RepositoryId::new(0)).await;
        let large_repo = linear::getrepo_with_id(fb, RepositoryId::new(1)).await;

        let master = BookmarkName::new("master")?;
        let maybe_master_val = small_repo.get_bonsai_bookmark(ctx.clone(), &master).await?;

        let master_val = maybe_master_val.ok_or(Error::msg("master not found"))?;
        let changesets =
            AncestorsNodeStream::new(ctx.clone(), &small_repo.get_changeset_fetcher(), master_val)
                .collect()
                .compat()
                .await?;

        let current_version = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
        for cs_id in changesets {
            mapping
                .add(
                    ctx.clone(),
                    SyncedCommitMappingEntry {
                        large_repo_id: large_repo.get_repoid(),
                        small_repo_id: small_repo.get_repoid(),
                        small_bcs_id: cs_id,
                        large_bcs_id: cs_id,
                        version_name: Some(current_version.clone()),
                    },
                )
                .compat()
                .await?;
        }

        let repos = match direction {
            CommitSyncDirection::LargeToSmall => CommitSyncRepos::LargeToSmall {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
            },
        };

        let commit_sync_data_provider = CommitSyncDataProvider::test_new(
            current_version.clone(),
            Source(repos.get_source_repo().get_repoid()),
            Target(repos.get_target_repo().get_repoid()),
            hashmap! {
                current_version => SyncData {
                    mover: Arc::new(identity_mover),
                    reverse_mover: Arc::new(identity_mover),
                    bookmark_renamer: Arc::new(noop_book_renamer),
                    reverse_bookmark_renamer: Arc::new(noop_book_renamer),
                }
            },
        );
        Ok(CommitSyncer::new_with_provider(
            &ctx,
            mapping,
            repos,
            commit_sync_data_provider,
        ))
    }
}
