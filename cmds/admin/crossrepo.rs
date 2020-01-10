/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use clap::{App, Arg, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::{future, future::IntoFuture, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};

use blobrepo::BlobRepo;
use bookmark_renaming::{get_large_to_small_renamer, get_small_to_large_renamer};
use bookmarks::BookmarkUpdateReason;
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use cross_repo_sync::{
    validation::{self, BookmarkDiff},
    CommitSyncRepos, CommitSyncer,
};
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt as PreviewFutureExt, TryFutureExt},
};
use metaconfig_types::{CommitSyncConfig, RepoConfig};
use movers::{get_large_to_small_mover, get_small_to_large_mover};
use slog::{info, warn, Logger};
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::error::SubcommandError;

const MAP_SUBCOMMAND: &str = "map";
const VERIFY_WC_SUBCOMMAND: &str = "verify-wc";
const VERIFY_BOOKMARKS_SUBCOMMAND: &str = "verify-bookmarks";
const HASH_ARG: &str = "HASH";
const LARGE_REPO_HASH_ARG: &str = "LARGE_REPO_HASH";
const UPDATE_LARGE_REPO_BOOKMARKS: &str = "update-large-repo-bookmarks";

pub fn subcommand_crossrepo(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let source_repo_id = try_boxfuture!(args::get_source_repo_id(fb, matches));
    let target_repo_id = try_boxfuture!(args::get_target_repo_id(fb, matches));

    args::init_cachelib(fb, &matches);
    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, matches);
    let target_repo = args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    // TODO(stash): in reality both source and target should point to the same mapping
    // It'll be nice to verify it
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(fb, &matches);

    match sub_m.subcommand() {
        (MAP_SUBCOMMAND, Some(sub_sub_m)) => {
            let hash = sub_sub_m.value_of(HASH_ARG).unwrap().to_owned();
            source_repo
                .join3(target_repo, mapping)
                .from_err()
                .and_then(move |(source_repo, target_repo, mapping)| {
                    subcommand_map(ctx, source_repo, target_repo, mapping, hash)
                })
                .boxify()
        }
        (VERIFY_WC_SUBCOMMAND, Some(sub_sub_m)) => {
            let (_, source_repo_config) =
                try_boxfuture!(args::get_config_by_repoid(fb, matches, source_repo_id));
            let large_hash = sub_sub_m.value_of(LARGE_REPO_HASH_ARG).unwrap().to_owned();

            source_repo
                .join3(target_repo, mapping)
                .from_err()
                .and_then(move |(source_repo, target_repo, mapping)| {
                    get_large_to_small_commit_sync_repos(
                        source_repo,
                        target_repo,
                        &source_repo_config,
                    )
                    .map(move |commit_sync_repos| CommitSyncer::new(mapping, commit_sync_repos))
                })
                .and_then({
                    cloned!(ctx);
                    move |commit_syncer| {
                        let large_repo = commit_syncer.get_large_repo();
                        helpers::csid_resolve(ctx.clone(), large_repo.clone(), large_hash)
                            .map(move |large_hash| (commit_syncer, large_hash))
                    }
                })
                .and_then(move |(commit_syncer, large_hash)| {
                    validation::verify_working_copy(ctx.clone(), commit_syncer, large_hash)
                        .boxed()
                        .compat()
                        .boxify()
                })
                .from_err()
                .boxify()
        }
        (VERIFY_BOOKMARKS_SUBCOMMAND, Some(sub_sub_m)) => {
            let (_, source_repo_config) =
                try_boxfuture!(args::get_config_by_repoid(fb, matches, source_repo_id));

            let update_large_repo_bookmarks = sub_sub_m.is_present(UPDATE_LARGE_REPO_BOOKMARKS);
            source_repo
                .join3(target_repo, mapping)
                .from_err()
                .and_then(move |(source_repo, target_repo, mapping)| {
                    subcommand_verify_bookmarks(
                        ctx,
                        source_repo,
                        source_repo_config,
                        target_repo,
                        mapping,
                        update_large_repo_bookmarks,
                    )
                    .boxed()
                    .compat()
                })
                .boxify()
        }
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}

fn subcommand_map(
    ctx: CoreContext,
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
    hash: String,
) -> BoxFuture<(), SubcommandError> {
    let source_repo_id = source_repo.get_repoid();
    let target_repo_id = target_repo.get_repoid();
    let source_hash = helpers::csid_resolve(ctx.clone(), source_repo, hash);
    source_hash
        .and_then(move |source_hash| {
            mapping
                .get(ctx.clone(), source_repo_id, source_hash, target_repo_id)
                .and_then(move |mapped| match mapped {
                    None => target_repo
                        .changeset_exists_by_bonsai(ctx, source_hash.clone())
                        .map(move |exists| {
                            if exists {
                                println!(
                                    "Hash {} not currently remapped (but present in target as-is)",
                                    source_hash
                                );
                            } else {
                                println!("Hash {} not currently remapped", source_hash);
                            }
                        })
                        .left_future(),
                    Some(target_hash) => {
                        println!("Hash {} maps to {}", source_hash, target_hash);
                        future::ok(()).right_future()
                    }
                })
        })
        .from_err()
        .boxify()
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
                        expected_target_cs_id,
                        actual_target_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "inconsistent value of {}: target repo has {}, but source repo cs remaps to {:?}",
                            target_bookmark,
                            expected_target_cs_id,
                            actual_target_cs_id,
                        );
                    }
                    ShouldBeDeleted { target_bookmark } => {
                        warn!(
                            ctx.logger(),
                            "target repo doesn't have bookmark {} but source repo has it",
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
                expected_target_cs_id,
                ..
            } => {
                let maybe_large_cs_id = mapping
                    .get(
                        ctx.clone(),
                        small_repo.get_repoid(),
                        *expected_target_cs_id,
                        large_repo.get_repoid(),
                    )
                    .compat()
                    .await?;

                if let Some(large_cs_id) = maybe_large_cs_id {
                    let reason = BookmarkUpdateReason::XRepoSync;
                    let large_bookmark = bookmark_renamer(&target_bookmark).ok_or(format_err!(
                        "small bookmark {} remaps to nothing",
                        target_bookmark
                    ))?;

                    info!(ctx.logger(), "setting {} {}", large_bookmark, large_cs_id);
                    book_txn.force_set(&large_bookmark, large_cs_id, reason)?;
                } else {
                    warn!(
                        ctx.logger(),
                        "{} from small repo doesn't remap to large repo", expected_target_cs_id,
                    );
                }
            }
            ShouldBeDeleted { target_bookmark } => {
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
                book_txn.force_delete(&large_bookmark, reason)?;
            }
        }
    }

    book_txn.commit().compat().await?;
    Ok(())
}

pub fn build_subcommand(name: &str) -> App {
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

    SubCommand::with_name(name)
        .subcommand(map_subcommand)
        .subcommand(verify_wc_subcommand)
        .subcommand(verify_bookmarks_subcommand)
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
            })
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use bookmarks::BookmarkName;
    use cross_repo_sync::validation::find_bookmark_diff;
    use fixtures::{linear, set_bookmark};
    use futures::stream::Stream;
    use maplit::{hashmap, hashset};
    use metaconfig_types::{
        CommitSyncConfig, CommitSyncDirection, DefaultSmallToLargeCommitSyncPathAction,
        SmallRepoCommitSyncConfig,
    };
    use mononoke_types::{MPath, RepositoryId};
    use revset::AncestorsNodeStream;
    use sql_ext::SqlConstructors;
    use std::{collections::HashSet, sync::Arc};
    // To support async tests
    use synced_commit_mapping::SyncedCommitMappingEntry;
    use tokio_preview as tokio;

    fn identity_mover(v: &MPath) -> Result<Option<MPath>, Error> {
        Ok(Some(v.clone()))
    }

    fn noop_book_renamer(bookmark_name: &BookmarkName) -> Option<BookmarkName> {
        Some(bookmark_name.clone())
    }

    #[fbinit::test]
    async fn test_bookmark_diff(fb: FacebookInit) -> Result<(), Error> {
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
        set_bookmark(fb, small_repo.clone(), another_hash, master.clone());
        let another_bcs_id =
            helpers::csid_resolve(ctx.clone(), small_repo.clone(), another_hash.to_string())
                .compat()
                .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

        let mut expected_diff = hashset! {
            BookmarkDiff::InconsistentValue {
                target_bookmark: master.clone(),
                expected_target_cs_id: another_bcs_id,
                actual_target_cs_id: Some(master_val),
            }
        };
        assert!(!actual_diff.is_empty());
        assert_eq!(
            actual_diff.into_iter().collect::<HashSet<_>>(),
            expected_diff,
        );

        // Create another bookmark
        let another_book = BookmarkName::new("newbook")?;
        set_bookmark(fb, small_repo.clone(), another_hash, another_book.clone());

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

        expected_diff.insert(BookmarkDiff::InconsistentValue {
            target_bookmark: another_book,
            expected_target_cs_id: another_bcs_id,
            actual_target_cs_id: None,
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
                    expected_target_cs_id: another_bcs_id,
                    actual_target_cs_id: Some(master_val),
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
        let small_repo = linear::getrepo_with_id(fb, RepositoryId::new(0));
        let large_repo = linear::getrepo_with_id(fb, RepositoryId::new(1));

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
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                reverse_mover: Arc::new(identity_mover),
                bookmark_renamer: Arc::new(noop_book_renamer),
                reverse_bookmark_renamer: Arc::new(noop_book_renamer),
            },
        };

        Ok(CommitSyncer::new(mapping, repos))
    }
}
