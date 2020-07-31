/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use bookmarks::{BookmarkName, Freshness};
use cached_config::ConfigStore;
use cmdlib::{args, helpers, monitoring};
use cmdlib_x_repo::{create_commit_syncer_from_matches, create_reverse_commit_syncer_from_matches};
use context::{CoreContext, SessionContainer};
use cross_repo_sync::{
    validation::{self, BookmarkDiff},
    CommitSyncOutcome, CommitSyncer, Syncers,
};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, future};
use futures_old::Stream;
use live_commit_sync_config::CONFIGERATOR_PUSHREDIRECT_ENABLE;
use mononoke_types::ChangesetId;
use pushredirect_enable::types::MononokePushRedirectEnable;
use scuba_ext::ScubaSampleBuilder;
use slog::{debug, error};
use stats::prelude::*;
use std::{sync::Arc, time::Duration};
use synced_commit_mapping::SyncedCommitMapping;

const CONFIGERATOR_TIMEOUT: Duration = Duration::from_millis(25);
const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);

define_stats! {
    prefix = "mononoke.bookmark_validator";
    result_counter: dynamic_singleton_counter(
        "{}.{}",
        (large_repo_name: String, small_repo_name: String)
    ),
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app_name = "Tool to validate that small and large repo bookmarks are in sync";
    let app = args::MononokeApp::new(app_name)
        .with_source_and_target_repos()
        .with_fb303_args()
        .build();
    let matches = app.get_matches();
    let (_, logger, mut runtime) = args::init_mononoke(fb, &matches, None)?;

    let large_to_small_commit_syncer =
        runtime.block_on_std(create_commit_syncer_from_matches(fb, &logger, &matches))?;

    if large_to_small_commit_syncer.get_source_repo().get_repoid()
        != large_to_small_commit_syncer.get_large_repo().get_repoid()
    {
        return Err(format_err!("Source repo must be a large repo!"));
    }

    // Backsyncer works in large -> small direction, however
    // for bookmarks vaidator it's simpler to have commits syncer in small -> large direction
    // Hence here we are creating a reverse syncer
    let small_to_large_commit_syncer = runtime.block_on_std(
        create_reverse_commit_syncer_from_matches(fb, &logger, &matches),
    )?;

    let syncers = Syncers {
        large_to_small: large_to_small_commit_syncer,
        small_to_large: small_to_large_commit_syncer,
    };

    let session_container = SessionContainer::new_with_defaults(fb);
    let scuba_sample = ScubaSampleBuilder::with_discard();
    let ctx = session_container.new_context(logger.clone(), scuba_sample);
    helpers::block_execute(
        loop_forever(ctx, syncers),
        fb,
        "megarepo_bookmarks_validator",
        &logger,
        &matches,
        monitoring::AliveService,
    )
}

async fn loop_forever<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    syncers: Syncers<M>,
) -> Result<(), Error> {
    let large_repo_name = syncers.large_to_small.get_large_repo().name();
    let small_repo_name = syncers.large_to_small.get_small_repo().name();

    let small_repo_id = syncers.small_to_large.get_small_repo().get_repoid();
    let config_store = ConfigStore::configerator(
        ctx.fb,
        ctx.logger().clone(),
        CONFIGERATOR_POLL_INTERVAL,
        CONFIGERATOR_TIMEOUT,
    )?;

    let config_handle =
        config_store.get_config_handle(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string())?;

    loop {
        let config: Arc<MononokePushRedirectEnable> = config_handle.get();

        let enabled = config
            .per_repo
            .get(&(small_repo_id.id() as i64))
            // We only care about public pushes because draft pushes are not in the bookmark
            // update log at all.
            .map(|enables| enables.public_push)
            .unwrap_or(false);

        if enabled {
            let res = validate(&ctx, &syncers, large_repo_name, small_repo_name).await;
            STATS::result_counter.set_value(
                ctx.fb,
                res.is_ok() as i64,
                (large_repo_name.clone(), small_repo_name.clone()),
            );

            if let Err(err) = res {
                error!(ctx.logger(), "validation failed: {:?}", err);
            }
        } else {
            debug!(ctx.logger(), "push redirector is disabled");
            // Log success to prevent alarm from going off
            STATS::result_counter.set_value(
                ctx.fb,
                1,
                (large_repo_name.clone(), small_repo_name.clone()),
            );
        }
        tokio::time::delay_for(Duration::new(1, 0)).await;
    }
}

async fn validate<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    syncers: &Syncers<M>,
    large_repo_name: &str,
    small_repo_name: &str,
) -> Result<(), Error> {
    let commit_syncer = &syncers.small_to_large;
    let diffs = validation::find_bookmark_diff(ctx.clone(), &commit_syncer).await?;

    debug!(ctx.logger(), "got {} bookmark diffs", diffs.len());
    for diff in diffs {
        debug!(ctx.logger(), "processing {:?}", diff);
        use BookmarkDiff::*;

        let (large_bookmark, large_cs_id, small_cs_id) = match diff {
            // Target is large, source is small here
            InconsistentValue {
                target_bookmark,
                target_cs_id,
                source_cs_id,
            } => (target_bookmark, Some(target_cs_id), source_cs_id),
            MissingInTarget {
                target_bookmark,
                source_cs_id,
            } => (target_bookmark, None, Some(source_cs_id)),
            NoSyncOutcome { target_bookmark } => {
                return Err(format_err!(
                    "unexpected no sync outcome for {}",
                    target_bookmark
                ));
            }
        };

        // Check that large_bookmark actually pointed to a commit equivalent to small_cs_id
        // not so long ago.
        let max_log_records: u32 = 100;
        let max_delay_secs: u32 = 300;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &large_bookmark,
            &small_cs_id,
            max_log_records,
            max_delay_secs,
        )
        .await?;
        if in_history {
            debug!(ctx.logger(), "all is well");
        } else {
            return Err(format_err!(
                "{} points to {:?} in {}, but points to {:?} in {}",
                large_bookmark,
                large_cs_id,
                large_repo_name,
                small_cs_id,
                small_repo_name,
            ));
        }
    }
    Ok(())
}

// Check that commit equivalent to maybe_small_cs_id was in large_bookmark log recently
async fn check_large_bookmark_history<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    syncers: &Syncers<M>,
    large_bookmark: &BookmarkName,
    maybe_small_cs_id: &Option<ChangesetId>,
    max_log_records: u32,
    max_delay_secs: u32,
) -> Result<bool, Error> {
    let small_to_large = &syncers.small_to_large;
    let large_to_small = &syncers.large_to_small;
    debug!(ctx.logger(), "checking history of {}", large_bookmark);

    let large_repo = small_to_large.get_large_repo();
    // Log entries are sorted newest to oldest
    let log_entries = large_repo
        .list_bookmark_log_entries(
            ctx.clone(),
            large_bookmark.clone(),
            max_log_records,
            None,
            Freshness::MostRecent,
        )
        .collect()
        .compat()
        .await?;

    if let Some((_, _, latest_timestamp)) = log_entries.get(0) {
        // Remap large repo commits into small repo commits
        // Note that in theory it's possible to map a small repo commit into a large repo and compare
        // only this remapped commit with the log of the large bookmark. However it doesn't work well
        // in practice - if two small repos are tailed into a large repo and one small repo is has
        // much more commits than the other, then latest max_log_records in the large repo might be
        // from the more active source repo. Hence check_large_bookmark_history might return 'false'
        // for the less active repo.
        let remapped_log_entries = log_entries
            .iter()
            .map(|(book_val, _, timestamp)| async move {
                let res: Result<_, Error> = match book_val {
                    Some(large_cs_id) => {
                        let maybe_remapped_cs_id =
                            remap(&ctx, &large_to_small, &large_cs_id).await?;
                        Ok(maybe_remapped_cs_id
                            .map(|remapped_cs_id| (Some(remapped_cs_id), timestamp)))
                    }
                    None => Ok(Some((None, timestamp))),
                };
                res
            });

        let remapped_log_entries = future::try_join_all(remapped_log_entries).await?;

        let maybe_log_entry = remapped_log_entries.into_iter().filter_map(|x| x).find(
            |(maybe_remapped_cs_id, timestamp)| {
                // Delay is measured from the latest entry in the large repo bookmark update log
                let delay = latest_timestamp.timestamp_seconds() - timestamp.timestamp_seconds();
                (maybe_remapped_cs_id == maybe_small_cs_id) && (delay < max_delay_secs as i64)
            },
        );

        if maybe_log_entry.is_some() {
            return Ok(true);
        }
    }
    // We haven't found an entry with the same id - check that bookmark might have
    // been created recently
    let was_created = log_entries.len() < (max_log_records as usize);
    if was_created && maybe_small_cs_id.is_none() {
        match log_entries.last() {
            Some((_, _, timestamp)) => Ok(timestamp.since_seconds() < max_delay_secs as i64),
            None => {
                // Shouldn't happen in practive, so return false in that case
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

async fn remap<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    source_cs_id: &ChangesetId,
) -> Result<Option<ChangesetId>, Error> {
    let maybe_commit_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx.clone(), *source_cs_id)
        .await?;

    use CommitSyncOutcome::*;

    match maybe_commit_sync_outcome {
        None | Some(NotSyncCandidate) => Ok(None),
        Some(RewrittenAs(cs_id, _)) | Some(EquivalentWorkingCopyAncestor(cs_id)) => Ok(Some(cs_id)),
        Some(Preserved) => Ok(Some(*source_cs_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cross_repo_sync_test_utils::init_small_large_repo;
    use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn simple_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        let ctx = CoreContext::test_mock(fb);
        runtime.block_on_std(test_simple_check_large_bookmark_history(ctx))
    }

    async fn test_simple_check_large_bookmark_history(ctx: CoreContext) -> Result<(), Error> {
        let (syncers, _) = init_small_large_repo(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let small_repo = small_to_large.get_small_repo();
        let small_master = resolve_cs_id(&ctx, small_repo, "master").await?;
        // Arbitrary large number
        let max_delay_secs = 10000000;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(small_master),
            100,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);

        // "master" moved, but it still in history
        let large_repo = small_to_large.get_large_repo();
        bookmark(&ctx, &large_repo, "master")
            .set_to("premove")
            .await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(small_master),
            100,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);

        // Now check with only one log record allowed - shouldn't be in history
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(small_master),
            1,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);

        // Create a new commit - not in master history
        let cs_id = CreateCommitContext::new(&ctx, small_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;

        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(cs_id),
            100,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);

        Ok(())
    }

    #[fbinit::test]
    fn another_repo_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        let ctx = CoreContext::test_mock(fb);
        runtime.block_on_std(test_another_repo_check_large_bookmark_history(ctx))
    }

    async fn test_another_repo_check_large_bookmark_history(ctx: CoreContext) -> Result<(), Error> {
        let (syncers, _) = init_small_large_repo(&ctx).await?;
        let small_to_large = &syncers.small_to_large;

        let small_repo = small_to_large.get_small_repo();
        let large_repo = small_to_large.get_large_repo();
        let small_master = resolve_cs_id(&ctx, small_repo, "master").await?;

        // Move master a few times in large with commits that do not remap to a small repo
        let mut last = None;
        for i in 1..10 {
            let cs_id = CreateCommitContext::new(&ctx, large_repo, vec!["master"])
                .add_file("somefile", format!("content{}", i))
                .commit()
                .await?;
            bookmark(&ctx, &large_repo, "master").set_to(cs_id).await?;
            last = Some(cs_id);
        }
        syncers
            .large_to_small
            .sync_commit(&ctx, last.unwrap())
            .await?;

        // Since all commits were from another repo, large repo's master still remaps
        // to small repo master, so it's in history
        let max_delay_secs = 10000000;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(small_master),
            1,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);

        // But now move a master a few times with commits that remap to small repo
        // (note "prefix/" in the filename).
        // In that case validation should fail.
        for i in 1..10 {
            let cs_id = CreateCommitContext::new(&ctx, large_repo, vec!["master"])
                .add_file("prefix/somefile", format!("content{}", i))
                .commit()
                .await?;
            bookmark(&ctx, &large_repo, "master").set_to(cs_id).await?;
        }
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &Some(small_master),
            1,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);
        Ok(())
    }

    #[fbinit::test]
    fn recently_created_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        let ctx = CoreContext::test_mock(fb);
        runtime.block_on_std(test_recently_created_check_large_bookmark_history(ctx))
    }

    async fn test_recently_created_check_large_bookmark_history(
        ctx: CoreContext,
    ) -> Result<(), Error> {
        let (syncers, _) = init_small_large_repo(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let large_repo = small_to_large.get_large_repo();

        // Move master a few times
        for i in 1..10 {
            let cs_id = CreateCommitContext::new(&ctx, large_repo, vec!["master"])
                .add_file("somefile", format!("content{}", i))
                .commit()
                .await?;
            bookmark(&ctx, &large_repo, "master").set_to(cs_id).await?;
        }

        bookmark(&ctx, &large_repo, "newbook")
            .set_to("master")
            .await?;

        // Bookmark was recently created - it's ok if it's not present in small repo
        let max_delay_secs = 10000000;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("newbook")?,
            &None,
            5,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);

        // However it's not ok for master to not be present in the repo
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &None,
            5,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);

        Ok(())
    }

    #[fbinit::test]
    fn deleted_added_back_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        let ctx = CoreContext::test_mock(fb);
        runtime.block_on_std(test_deleted_added_back_created_check_large_bookmark_history(ctx))
    }

    async fn test_deleted_added_back_created_check_large_bookmark_history(
        ctx: CoreContext,
    ) -> Result<(), Error> {
        let (syncers, _) = init_small_large_repo(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let large_repo = small_to_large.get_large_repo();

        bookmark(&ctx, &large_repo, "master").delete().await?;
        bookmark(&ctx, &large_repo, "master")
            .set_to("premove")
            .await?;

        // Recently deleted - should be in history
        let max_delay_secs = 10000000;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkName::new("master")?,
            &None,
            2,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);
        Ok(())
    }
}
