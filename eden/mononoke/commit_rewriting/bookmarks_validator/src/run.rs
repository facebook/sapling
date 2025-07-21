/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use blobstore_factory::MetadataSqlFactory;
use bookmarks::BookmarkKey;
use bookmarks::Freshness;
use context::CoreContext;
use cross_repo_sync::BookmarkDiff;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::Repo as CrossRepo;
use cross_repo_sync::Syncers;
use cross_repo_sync::find_bookmark_diff;
use environment::MononokeEnvironment;
use futures::TryStreamExt;
use futures::future;
use mononoke_types::ChangesetId;
use pushredirect::PushRedirectionConfig;
use pushredirect::SqlPushRedirectionConfigBuilder;
use slog::error;
use slog::info;
use stats::prelude::*;

define_stats! {
  prefix = "mononoke.bookmark_validator";
  result_counter: dynamic_singleton_counter(
      "{}.{}",
      (large_repo_name: String, small_repo_name: String)
  ),
}

pub(crate) async fn loop_forever<R: CrossRepo>(
    ctx: &CoreContext,
    env: &Arc<MononokeEnvironment>,
    syncers: Syncers<R>,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let large_repo = syncers.large_to_small.get_large_repo();
    let small_repo = syncers.large_to_small.get_small_repo();
    let large_repo_name = large_repo.repo_identity().name();
    let small_repo_name = small_repo.repo_identity().name();

    let small_repo_id = syncers.small_to_large.get_small_repo().repo_identity().id();

    let small_repo_config = small_repo.repo_config();
    let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
        ctx.fb,
        small_repo_config.storage_config.metadata.clone(),
        env.mysql_options.clone(),
        blobstore_factory::ReadOnlyStorage(env.readonly_storage.0),
    )
    .await?;
    let builder = sql_factory
        .open::<SqlPushRedirectionConfigBuilder>()
        .await?;
    let push_redirection_config = builder.build(small_repo.sql_query_config_arc());

    loop {
        // Before initiating every iteration, check if cancellation has been requested.
        if cancellation_requested.load(Ordering::Relaxed) {
            info!(
                ctx.logger(),
                "bookmark validation stopping due to cancellation request"
            );
            return Ok(());
        }

        let enabled = push_redirection_config
            .get(ctx, small_repo_id)
            .await?
            .is_some_and(|enables| enables.public_push);

        if enabled {
            let res = validate(ctx, &syncers, large_repo_name, small_repo_name).await;
            if let Err(err) = res {
                match err {
                    ValidationError::InfraError(error) => {
                        error!(ctx.logger(), "infra error: {:?}", error);
                    }
                    ValidationError::ValidationError(err_msg) => {
                        STATS::result_counter.set_value(
                            ctx.fb,
                            0,
                            (large_repo_name.to_string(), small_repo_name.to_string()),
                        );
                        error!(ctx.logger(), "validation failed: {:?}", err_msg);
                    }
                }
            } else {
                STATS::result_counter.set_value(
                    ctx.fb,
                    1,
                    (large_repo_name.to_string(), small_repo_name.to_string()),
                );
            }
        } else {
            info!(ctx.logger(), "push redirector is disabled");
            // Log success to prevent alarm from going off
            STATS::result_counter.set_value(
                ctx.fb,
                1,
                (large_repo_name.to_string(), small_repo_name.to_string()),
            );
        }
        tokio::time::sleep(Duration::from_millis(justknobs::get_as::<u64>(
            "scm/mononoke:bookmarks_validator_sleep_ms",
            None,
        )?))
        .await;
    }
}

enum ValidationError {
    InfraError(Error),
    ValidationError(String),
}

impl From<Error> for ValidationError {
    fn from(error: Error) -> Self {
        Self::InfraError(error)
    }
}

async fn validate<R: CrossRepo>(
    ctx: &CoreContext,
    syncers: &Syncers<R>,
    large_repo_name: &str,
    small_repo_name: &str,
) -> Result<(), ValidationError> {
    let commit_sync_data = &syncers.small_to_large;
    let diffs = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;

    info!(ctx.logger(), "got {} bookmark diffs", diffs.len());
    for diff in diffs {
        info!(ctx.logger(), "processing {:?}", diff);
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
                return Err(ValidationError::ValidationError(format!(
                    "unexpected no sync outcome for {}",
                    target_bookmark
                )));
            }
        };

        // Check that large_bookmark actually pointed to a commit equivalent to small_cs_id
        // not so long ago.
        let max_log_records =
            justknobs::get_as::<u32>("scm/mononoke:bookmarks_validator_max_log_records", None)?;
        let max_delay_secs: u32 = 300;
        let in_history = check_large_bookmark_history(
            ctx,
            syncers,
            &large_bookmark,
            &large_cs_id,
            &small_cs_id,
            max_log_records,
            max_delay_secs,
        )
        .await?;
        if in_history {
            info!(ctx.logger(), "all is well");
        } else {
            let err_msg = format!(
                "{} points to {:?} in {}, but points to {:?} in {}",
                large_bookmark, large_cs_id, large_repo_name, small_cs_id, small_repo_name,
            );
            return Err(ValidationError::ValidationError(err_msg));
        }
    }
    Ok(())
}

// Check that commit equivalent to maybe_small_cs_id was in large_bookmark log recently
async fn check_large_bookmark_history<R: CrossRepo>(
    ctx: &CoreContext,
    syncers: &Syncers<R>,
    large_bookmark: &BookmarkKey,
    maybe_large_cs_id: &Option<ChangesetId>,
    maybe_small_cs_id: &Option<ChangesetId>,
    max_log_records: u32,
    max_delay_secs: u32,
) -> Result<bool, Error> {
    let small_to_large = &syncers.small_to_large;
    let large_to_small = &syncers.large_to_small;
    info!(ctx.logger(), "checking history of {}", large_bookmark);

    let large_repo = small_to_large.get_large_repo();
    // Log entries are sorted newest to oldest
    let log_entries: Vec<_> = large_repo
        .bookmark_update_log()
        .list_bookmark_log_entries(
            ctx.clone(),
            large_bookmark.clone(),
            max_log_records,
            None,
            Freshness::MostRecent,
        )
        .try_collect()
        .await?;

    let maybe_large_bookmark_log_entry = log_entries
        .iter()
        .find(|(_, book_val, _, _)| book_val == maybe_large_cs_id);

    let large_bookmark_timestamp = match maybe_large_bookmark_log_entry {
        Some((_, _, _, timestamp)) => timestamp,
        // We can't find the large bookmark in bookmark update log.
        None => return Ok(false),
    };

    // Remap large repo commits into small repo commits
    // Note that in theory it's possible to map a small repo commit into a large repo and compare
    // only this remapped commit with the log of the large bookmark. However it doesn't work well
    // in practice - if two small repos are tailed into a large repo and one small repo is has
    // much more commits than the other, then latest max_log_records in the large repo might be
    // from the more active source repo. Hence check_large_bookmark_history might return 'false'
    // for the less active repo.
    let remapped_log_entries = log_entries
        .iter()
        .map(|(_, book_val, _, timestamp)| async move {
            let res: Result<_, Error> =
                match book_val {
                    Some(large_cs_id) => {
                        let maybe_remapped_cs_id = remap(ctx, large_to_small, large_cs_id).await?;
                        Ok(maybe_remapped_cs_id
                            .map(|remapped_cs_id| (Some(remapped_cs_id), timestamp)))
                    }
                    None => Ok(Some((None, timestamp))),
                };
            res
        });

    let remapped_log_entries = future::try_join_all(remapped_log_entries).await?;

    let maybe_log_entry = remapped_log_entries
        .into_iter()
        .filter_map(std::convert::identity)
        .find(|(maybe_remapped_cs_id, timestamp)| {
            // Delay is measured from the large bookmark entry in the large repo bookmark update log.
            // This log entry could be more recent than the entry pointing to large_bookmark, in which
            // case the delay would be negative and the condition would evaluate to true. This is fine
            // as we only want to exclude entries that are too old.
            let delay =
                large_bookmark_timestamp.timestamp_seconds() - timestamp.timestamp_seconds();
            (maybe_remapped_cs_id == maybe_small_cs_id) && (delay < max_delay_secs as i64)
        });

    if maybe_log_entry.is_some() {
        return Ok(true);
    }

    // We haven't found an entry with the same id - check that bookmark might have
    // been created recently
    let was_created = log_entries.len() < (max_log_records as usize);
    if was_created && maybe_small_cs_id.is_none() {
        match log_entries.last() {
            Some((_, _, _, timestamp)) => Ok(timestamp.since_seconds() < max_delay_secs as i64),
            None => {
                // Shouldn't happen in practice, so return false in that case
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

async fn remap<R: CrossRepo>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    source_cs_id: &ChangesetId,
) -> Result<Option<ChangesetId>, Error> {
    let maybe_commit_sync_outcome = commit_sync_data
        .get_commit_sync_outcome(ctx, *source_cs_id)
        .await?;

    use CommitSyncOutcome::*;

    match maybe_commit_sync_outcome {
        None | Some(NotSyncCandidate(_)) => Ok(None),
        Some(RewrittenAs(cs_id, _)) | Some(EquivalentWorkingCopyAncestor(cs_id, _)) => {
            Ok(Some(cs_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use cross_repo_sync::CandidateSelectionHint;
    use cross_repo_sync::CommitSyncContext;
    use cross_repo_sync::sync_commit;
    use cross_repo_sync::test_utils::TestRepo;
    use cross_repo_sync::test_utils::init_small_large_repo;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use mononoke_types::DateTime;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;
    use tests_utils::resolve_cs_id;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_simple_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let large_repo = small_to_large.get_large_repo();
        let small_repo = small_to_large.get_small_repo();
        let small_master = resolve_cs_id(&ctx, small_repo, "master").await?;
        // Arbitrary large number
        let max_delay_secs = 10000000;
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(large_master),
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
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(large_master),
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
            &BookmarkKey::new("master")?,
            &Some(large_master),
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
            &BookmarkKey::new("master")?,
            &Some(large_master),
            &Some(cs_id),
            100,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_another_repo_check_large_bookmark_history(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
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

        sync_commit(
            &ctx,
            last.unwrap(),
            &syncers.large_to_small,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
            false,
        )
        .await?;

        // Since all commits were from another repo, large repo's master still remaps
        // to small repo master, so it's in history
        let max_delay_secs = 10000000;
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(large_master),
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
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(large_master),
            &Some(small_master),
            1,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_recently_created_check_large_bookmark_history(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
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
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("newbook")?,
            &Some(large_master),
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
            &BookmarkKey::new("master")?,
            &Some(large_master),
            &None,
            5,
            max_delay_secs,
        )
        .await?;
        assert!(!in_history);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_deleted_added_back_created_check_large_bookmark_history(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let large_repo = small_to_large.get_large_repo();

        bookmark(&ctx, &large_repo, "master").delete().await?;
        bookmark(&ctx, &large_repo, "master")
            .set_to("premove")
            .await?;

        // Recently deleted - should be in history
        let max_delay_secs = 10000000;
        let large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(large_master),
            &None,
            2,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_check_large_bookmark_history_after_bookmark_moved(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo::<TestRepo>(&ctx).await?;
        let small_to_large = &syncers.small_to_large;
        let small_repo = small_to_large.get_small_repo();
        let large_repo = small_to_large.get_large_repo();

        let small_master = resolve_cs_id(&ctx, small_repo, "master").await?;

        // Wait a little bit
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Move a bookmark in the large repo
        let old_large_master = resolve_cs_id(&ctx, large_repo, "master").await?;
        let new_master = CreateCommitContext::new(&ctx, &large_repo, vec![old_large_master])
            .add_file("prefix/somefile", "somecontent")
            .set_author_date(DateTime::now())
            .commit()
            .await?;
        bookmark(&ctx, &large_repo, "master")
            .set_to(new_master)
            .await?;

        sync_commit(
            &ctx,
            new_master,
            &syncers.large_to_small,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
            false,
        )
        .await?;

        let max_delay_secs = 1;
        let in_history = check_large_bookmark_history(
            &ctx,
            &syncers,
            &BookmarkKey::new("master")?,
            &Some(old_large_master),
            &Some(small_master),
            100,
            max_delay_secs,
        )
        .await?;
        assert!(in_history);

        Ok(())
    }
}
