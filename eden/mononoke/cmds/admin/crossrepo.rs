/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use bookmark_renaming::{get_large_to_small_renamer, get_small_to_large_renamer};
use bookmarks::BookmarkUpdateReason;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{args, helpers};
use context::CoreContext;
use cross_repo_sync::{
    validation::{self, BookmarkDiff},
    CommitSyncRepos, CommitSyncer,
};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, try_join};
use futures_ext::FutureExt;
use itertools::Itertools;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::{CommitSyncConfig, RepoConfig};
use mononoke_types::RepositoryId;
use movers::{get_large_to_small_mover, get_small_to_large_mover};
use slog::{info, warn, Logger};
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::error::SubcommandError;

pub const CROSSREPO: &str = "crossrepo";
const MAP_SUBCOMMAND: &str = "map";
const VERIFY_WC_SUBCOMMAND: &str = "verify-wc";
const VERIFY_BOOKMARKS_SUBCOMMAND: &str = "verify-bookmarks";
const HASH_ARG: &str = "HASH";
const LARGE_REPO_HASH_ARG: &str = "LARGE_REPO_HASH";
const UPDATE_LARGE_REPO_BOOKMARKS: &str = "update-large-repo-bookmarks";

const SUBCOMMAND_CONFIG: &str = "config";
const SUBCOMMAND_BY_VERSION: &str = "by-version";
const SUBCOMMAND_LIST: &str = "list";
const SUBCOMMAND_CURRENT: &str = "current";
const ARG_VERSION_NAME: &str = "version-name";
const ARG_WITH_CONTENTS: &str = "with-contents";

pub async fn subcommand_crossrepo<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    args::init_cachelib(fb, &matches, None);
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
            let source_repo_id = source_repo.get_repoid();

            let (_, source_repo_config) = args::get_config_by_repoid(fb, matches, source_repo_id)?;

            let commit_syncer = {
                let commit_sync_repos = get_large_to_small_commit_sync_repos(
                    source_repo,
                    target_repo,
                    &source_repo_config,
                )?;
                CommitSyncer::new(mapping, commit_sync_repos)
            };

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
            let (source_repo, target_repo, mapping) =
                get_source_target_repos_and_mapping(fb, logger, matches).await?;
            let source_repo_id = source_repo.get_repoid();

            let (_, source_repo_config) = args::get_config_by_repoid(fb, matches, source_repo_id)?;

            let update_large_repo_bookmarks = sub_sub_m.is_present(UPDATE_LARGE_REPO_BOOKMARKS);

            subcommand_verify_bookmarks(
                ctx,
                source_repo,
                source_repo_config,
                target_repo,
                mapping,
                update_large_repo_bookmarks,
            )
            .await
        }
        (SUBCOMMAND_CONFIG, Some(sub_sub_m)) => {
            run_config_sub_subcommand(fb, ctx, logger, matches, sub_sub_m).await
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn run_config_sub_subcommand<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    config_subcommand_matches: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let repo_id = args::get_repo_id(fb, matches)?;
    let config_store = args::maybe_init_config_store(fb, &logger, &matches)
        .expect("failed to instantiate ConfigStore");
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;

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

async fn get_source_target_repos_and_mapping<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
) -> Result<(BlobRepo, BlobRepo, SqlSyncedCommitMapping), Error> {
    let source_repo_id = args::get_source_repo_id(fb, matches)?;
    let target_repo_id = args::get_target_repo_id(fb, matches)?;

    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, matches)
        .boxify()
        .compat();
    let target_repo = args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches)
        .boxify()
        .compat();
    // TODO(stash): in reality both source and target should point to the same mapping
    // It'll be nice to verify it
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(fb, &matches).compat();

    try_join!(source_repo, target_repo, mapping)
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

    let mapped = mapping
        .get(ctx.clone(), source_repo_id, source_hash, target_repo_id)
        .compat()
        .await?;

    match mapped {
        None => {
            let exists = target_repo
                .changeset_exists_by_bonsai(ctx, source_hash.clone())
                .compat()
                .await?;

            if exists {
                println!(
                    "Hash {} not currently remapped (but present in target as-is)",
                    source_hash
                );
            } else {
                println!("Hash {} not currently remapped", source_hash);
            }
        }

        Some((target_hash, maybe_version_name)) => match maybe_version_name {
            Some(version_name) => {
                println!(
                    "Hash {} maps to {}, used {:?}",
                    source_hash, target_hash, version_name
                );
            }
            None => {
                println!("Hash {} maps to {}", source_hash, target_hash);
            }
        },
    };

    Ok(())
}

async fn subcommand_verify_bookmarks(
    ctx: CoreContext,
    source_repo: BlobRepo,
    source_repo_config: RepoConfig,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
    should_update_large_repo_bookmarks: bool,
) -> Result<(), SubcommandError> {
    let commit_sync_repos = get_large_to_small_commit_sync_repos(
        source_repo.clone(),
        target_repo.clone(),
        &source_repo_config,
    )?;
    let commit_syncer = CommitSyncer::new(mapping.clone(), commit_sync_repos);

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
                            "inconsistent value of {}: target repo has {}, but source repo bookmark points to {:?}",
                            target_bookmark,
                            target_cs_id,
                            source_cs_id,
                        );
                    }
                    MissingInTarget {
                        target_bookmark,
                        source_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "target repo doesn't have bookmark {} but source repo has it and it points to {}",
                            target_bookmark,
                            source_cs_id,
                        );
                    }
                    NoSyncOutcome { target_bookmark } => {
                        warn!(
                            ctx.logger(),
                            "target repo has a bookmark {} but it points to a commit that has no \
                             equivalent in source repo. If it's a shared bookmark (e.g. master) \
                             that might mean that it points to a commit from another repository",
                            target_bookmark,
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
                let maybe_large_cs_id = mapping
                    .get(
                        ctx.clone(),
                        small_repo.get_repoid(),
                        *target_cs_id,
                        large_repo.get_repoid(),
                    )
                    .compat()
                    .await?;

                if let Some((large_cs_id, _)) = maybe_large_cs_id {
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

    SubCommand::with_name(CROSSREPO)
        .subcommand(map_subcommand)
        .subcommand(verify_wc_subcommand)
        .subcommand(verify_bookmarks_subcommand)
        .subcommand(commit_sync_config_subcommand)
}

fn get_large_to_small_commit_sync_repos(
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    repo_config: &RepoConfig,
) -> Result<CommitSyncRepos, Error> {
    repo_config
        .commit_sync_config
        .as_ref()
        .ok_or_else(|| format_err!("missing CommitSyncMapping config"))
        .and_then(|commit_sync_config| {
            let (large_repo, small_repo) = if commit_sync_config.large_repo_id
                == source_repo.get_repoid()
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

            let bookmark_renamer =
                get_large_to_small_renamer(commit_sync_config, small_repo.get_repoid())?;
            let reverse_bookmark_renamer =
                get_small_to_large_renamer(commit_sync_config, small_repo.get_repoid())?;
            let mover = get_large_to_small_mover(&commit_sync_config, small_repo.get_repoid())?;
            let reverse_mover =
                get_small_to_large_mover(&commit_sync_config, small_repo.get_repoid())?;

            Ok(CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                mover,
                reverse_mover,
                bookmark_renamer,
                reverse_bookmark_renamer,
                version_name: commit_sync_config.version_name.clone(),
            })
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use bookmarks::BookmarkName;
    use cross_repo_sync::validation::find_bookmark_diff;
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

    fn identity_mover(v: &MPath) -> Result<Option<MPath>, Error> {
        Ok(Some(v.clone()))
    }

    fn noop_book_renamer(bookmark_name: &BookmarkName) -> Option<BookmarkName> {
        Some(bookmark_name.clone())
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
        let maybe_master_val = small_repo
            .get_bonsai_bookmark(ctx.clone(), &master)
            .compat()
            .await?;
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
        let maybe_master_val = small_repo
            .get_bonsai_bookmark(ctx.clone(), &master)
            .compat()
            .await?;

        let master_val = maybe_master_val.ok_or(Error::msg("master not found"))?;
        let changesets =
            AncestorsNodeStream::new(ctx.clone(), &small_repo.get_changeset_fetcher(), master_val)
                .collect()
                .compat()
                .await?;

        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        for cs_id in changesets {
            mapping
                .add(
                    ctx.clone(),
                    SyncedCommitMappingEntry {
                        large_repo_id: large_repo.get_repoid(),
                        small_repo_id: small_repo.get_repoid(),
                        small_bcs_id: cs_id,
                        large_bcs_id: cs_id,
                        version_name: None,
                    },
                )
                .compat()
                .await?;
        }

        let repos = match direction {
            CommitSyncDirection::LargeToSmall => CommitSyncRepos::LargeToSmall {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                reverse_mover: Arc::new(identity_mover),
                bookmark_renamer: Arc::new(noop_book_renamer),
                reverse_bookmark_renamer: Arc::new(noop_book_renamer),
                version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                reverse_mover: Arc::new(identity_mover),
                bookmark_renamer: Arc::new(noop_book_renamer),
                reverse_bookmark_renamer: Arc::new(noop_book_renamer),
                version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            },
        };

        Ok(CommitSyncer::new(mapping, repos))
    }
}
