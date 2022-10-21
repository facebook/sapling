/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateReason;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcherArc;
use cloned::cloned;
use context::CoreContext;
use futures::compat::Future01CompatExt;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures_watchdog::WatchdogExt;
use getbundle_response::SessionLfsParams;
use itertools::Itertools;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use metaconfig_types::RepoConfigRef;
use mononoke_hg_sync_job_helper_lib::save_bytes_to_temp_file;
use mononoke_hg_sync_job_helper_lib::write_to_named_temp_file;
use mononoke_types::datetime::Timestamp;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use regex::Regex;
use slog::info;
use slog::warn;
use tempfile::NamedTempFile;

use crate::bind_sync_err;
use crate::bundle_generator::BookmarkChange;
use crate::bundle_generator::FilenodeVerifier;
use crate::errors::ErrorKind::BookmarkMismatchInBundleCombining;
use crate::errors::ErrorKind::UnexpectedBookmarkMove;
use crate::errors::PipelineError;
use crate::BookmarkOverlay;
use crate::CombinedBookmarkUpdateLogEntry;
use crate::CommitsInBundle;
use crate::Repo;

#[derive(Clone)]
pub struct PreparedBookmarkUpdateLogEntry {
    pub log_entry: BookmarkUpdateLogEntry,
    pub bundle_file: Arc<NamedTempFile>,
    pub timestamps_file: Arc<NamedTempFile>,
    pub cs_id: Option<(ChangesetId, HgChangesetId)>,
}

pub struct BundlePreparer {
    repo: Repo,
    base_retry_delay_ms: u64,
    retry_num: usize,
    filenode_verifier: FilenodeVerifier,
    bookmark_regex_force_lfs: Option<Regex>,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
}

impl BundlePreparer {
    pub async fn new_generate_bundles(
        repo: Repo,
        base_retry_delay_ms: u64,
        retry_num: usize,
        filenode_verifier: FilenodeVerifier,
        bookmark_regex_force_lfs: Option<Regex>,
        push_vars: Option<HashMap<String, bytes::Bytes>>,
    ) -> Result<BundlePreparer, Error> {
        Ok(BundlePreparer {
            repo,
            base_retry_delay_ms,
            retry_num,
            filenode_verifier,
            bookmark_regex_force_lfs,
            push_vars,
        })
    }

    pub async fn prepare_batches(
        &self,
        ctx: &CoreContext,
        overlay: &mut BookmarkOverlay,
        entries: Vec<BookmarkUpdateLogEntry>,
    ) -> Result<Vec<BookmarkLogEntryBatch>, Error> {
        use BookmarkUpdateReason::*;

        for log_entry in &entries {
            match log_entry.reason {
                Pushrebase | Backsyncer | ManualMove | ApiRequest | XRepoSync | Push | TestMove => {
                }
                Blobimport => {
                    return Err(UnexpectedBookmarkMove(format!("{}", log_entry.reason)).into());
                }
            };
        }

        split_in_batches(
            ctx,
            &self.repo.skiplist_index,
            &self.repo.changeset_fetcher_arc(),
            overlay,
            entries,
        )
        .await
    }

    pub fn prepare_bundles(
        &self,
        ctx: CoreContext,
        batches: Vec<BookmarkLogEntryBatch>,
    ) -> BoxFuture<'static, Result<Vec<CombinedBookmarkUpdateLogEntry>, PipelineError>> {
        let mut futs = vec![];

        for batch in batches {
            if batch.is_empty() {
                continue;
            }
            let session_lfs_params = self.session_lfs_params(&ctx, &batch.bookmark_name);
            let entries = batch.entries.clone();
            let f = self.prepare_single_bundle(ctx.clone(), batch, session_lfs_params);
            futs.push((f, entries));
        }

        let futs = futs
            .into_iter()
            .map(|(f, entries)| {
                let ctx = ctx.clone();
                async move {
                    let f = tokio::spawn(f);
                    let res = f.map_err(Error::from).watched(ctx.logger()).await;
                    let res = match res {
                        Ok(Ok(res)) => Ok(res),
                        Ok(Err(err)) => Err(err),
                        Err(err) => Err(err),
                    };
                    res.map_err(|err| bind_sync_err(&entries, err))
                }
            })
            .collect::<Vec<_>>();
        async move { try_join_all(futs).await }.boxed()
    }

    // Prepares a bundle that might be a result of combining a few BookmarkUpdateLogEntry.
    // Note that these entries should all move the same bookmark.
    fn prepare_single_bundle(
        &self,
        ctx: CoreContext,
        batch: BookmarkLogEntryBatch,
        session_lfs_params: SessionLfsParams,
    ) -> BoxFuture<'static, Result<CombinedBookmarkUpdateLogEntry, Error>> {
        cloned!(self.repo, self.push_vars, self.filenode_verifier);

        let base_retry_delay_ms = self.base_retry_delay_ms;
        let retry_num = self.retry_num;
        async move {
            let entry_ids = batch
                .entries
                .iter()
                .map(|log_entry| log_entry.id)
                .collect::<Vec<_>>();
            info!(ctx.logger(), "preparing log entry ids #{:?} ...", entry_ids);
            // Check that all entries modify bookmark_name
            for entry in &batch.entries {
                if entry.bookmark_name != batch.bookmark_name {
                    return Err(BookmarkMismatchInBundleCombining {
                        ids: entry_ids,
                        entry_id: entry.id,
                        entry_bookmark_name: entry.bookmark_name.clone(),
                        bundle_bookmark_name: batch.bookmark_name,
                    }
                    .into());
                }
            }

            let bookmark_change = BookmarkChange::new(batch.from_cs_id, batch.to_cs_id)?;
            let bundle_timestamps_commits = retry::retry_always(
                ctx.logger(),
                {
                    |_| {
                        Self::try_prepare_bundle_and_timestamps_file(
                            &ctx,
                            &repo,
                            &filenode_verifier,
                            session_lfs_params.clone(),
                            &batch.server_heads,
                            &bookmark_change,
                            &batch.bookmark_name,
                            push_vars.clone(),
                        )
                    }
                },
                base_retry_delay_ms,
                retry_num,
            )
            .map_ok(|(res, _)| res);

            let cs_id = async {
                match batch.to_cs_id {
                    Some(to_changeset_id) => {
                        let hg_cs_id = repo.derive_hg_changeset(&ctx, to_changeset_id).await?;
                        Ok(Some((to_changeset_id, hg_cs_id)))
                    }
                    None => Ok(None),
                }
            };

            let ((bundle_file, timestamps_file, commits), cs_id) =
                try_join(bundle_timestamps_commits, cs_id).await?;

            info!(
                ctx.logger(),
                "successful prepare of entries #{:?}", entry_ids
            );

            Ok(CombinedBookmarkUpdateLogEntry {
                components: batch.entries,
                bundle_file: Arc::new(bundle_file),
                timestamps_file: Arc::new(timestamps_file),
                cs_id,
                bookmark: batch.bookmark_name,
                commits,
            })
        }
        .boxed()
    }

    async fn try_prepare_bundle_and_timestamps_file<'a>(
        ctx: &'a CoreContext,
        repo: &'a Repo,
        filenode_verifier: &'a FilenodeVerifier,
        session_lfs_params: SessionLfsParams,
        hg_server_heads: &'a [ChangesetId],
        bookmark_change: &'a BookmarkChange,
        bookmark_name: &'a BookmarkName,
        push_vars: Option<HashMap<String, bytes::Bytes>>,
    ) -> Result<(NamedTempFile, NamedTempFile, CommitsInBundle), Error> {
        let (bytes, timestamps) = crate::bundle_generator::create_bundle(
            ctx.clone(),
            repo.clone(),
            bookmark_name.clone(),
            bookmark_change.clone(),
            hg_server_heads.to_vec(),
            session_lfs_params,
            filenode_verifier.clone(),
            push_vars,
        )
        .compat()
        .await?;

        let mut bcs_ids = vec![];
        for (hg_cs_id, (bcs_id, _)) in &timestamps {
            bcs_ids.push((*hg_cs_id, *bcs_id));
        }

        let timestamps = timestamps
            .into_iter()
            .map(|(hg_cs_id, (_, timestamp))| (hg_cs_id, timestamp))
            .collect();
        let (bundle, timestamps) = try_join(
            save_bytes_to_temp_file(&bytes),
            save_timestamps_to_file(&timestamps),
        )
        .await?;
        Ok((bundle, timestamps, CommitsInBundle::Commits(bcs_ids)))
    }

    fn session_lfs_params(&self, ctx: &CoreContext, bookmark: &BookmarkName) -> SessionLfsParams {
        let lfs_params = &self.repo.repo_config().lfs;

        if let Some(regex) = &self.bookmark_regex_force_lfs {
            if regex.is_match(bookmark.as_str()) {
                info!(ctx.logger(), "force generating lfs bundle for {}", bookmark);
                return SessionLfsParams {
                    threshold: lfs_params.threshold,
                };
            }
        }

        if lfs_params.generate_lfs_blob_in_hg_sync_job {
            SessionLfsParams {
                threshold: lfs_params.threshold,
            }
        } else {
            SessionLfsParams { threshold: None }
        }
    }
}

async fn save_timestamps_to_file(
    timestamps: &HashMap<HgChangesetId, Timestamp>,
) -> Result<NamedTempFile, Error> {
    let encoded_timestamps = timestamps
        .iter()
        .map(|(key, value)| {
            let timestamp = value.timestamp_seconds();
            format!("{}={}", key, timestamp)
        })
        .join("\n");

    write_to_named_temp_file(encoded_timestamps).await
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkLogEntryBatch {
    entries: Vec<BookmarkUpdateLogEntry>,
    bookmark_name: BookmarkName,
    from_cs_id: Option<ChangesetId>,
    to_cs_id: Option<ChangesetId>,
    server_old_value: Option<ChangesetId>,
    server_heads: Vec<ChangesetId>,
}

impl BookmarkLogEntryBatch {
    pub fn new(overlay: &mut BookmarkOverlay, log_entry: BookmarkUpdateLogEntry) -> Self {
        let bookmark_name = log_entry.bookmark_name.clone();
        let from_cs_id = log_entry.from_changeset_id;
        let to_cs_id = log_entry.to_changeset_id;
        let server_heads = overlay.all_values();
        let server_old_value = overlay.get(&bookmark_name);
        overlay.update(bookmark_name.clone(), to_cs_id.clone());
        Self {
            entries: vec![log_entry],
            bookmark_name,
            from_cs_id,
            to_cs_id,
            server_old_value,
            server_heads,
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // Outer result's error means that some infrastructure error happened.
    // Inner result's error means that it wasn't possible to append entry,
    // and this entry is returned as the error.
    pub async fn try_append(
        &mut self,
        ctx: &CoreContext,
        lca_hint: &dyn LeastCommonAncestorsHint,
        changeset_fetcher: &ArcChangesetFetcher,
        overlay: &mut BookmarkOverlay,
        entry: BookmarkUpdateLogEntry,
    ) -> Result<Result<(), BookmarkUpdateLogEntry>, Error> {
        // Combine two bookmark update log entries only if bookmark names are the same
        if self.bookmark_name != entry.bookmark_name {
            return Ok(Err(entry));
        }

        // if it's a non-fast forward move then put it in the separate batch.
        // Otherwise some of the commits might not be synced to hg servers.
        // Consider this case:
        // C
        // |
        // B
        // |
        // A
        //
        // 1 entry - moves a bookmark from A to C
        // 2 entry - moves a bookmark from C to B
        //
        // if we combine them together then we get a batch that
        // moves a bookmark from A to B and commit C won't be synced
        // to hg servers. To prevent that let's put non-fast forward
        // moves to a separate branch
        match (entry.from_changeset_id, entry.to_changeset_id) {
            (Some(from_cs_id), Some(to_cs_id)) => {
                let is_ancestor = lca_hint
                    .is_ancestor(ctx, changeset_fetcher, from_cs_id, to_cs_id)
                    .watched(ctx.logger())
                    .await?;
                if !is_ancestor {
                    // Force non-forward moves to go to a separate batch
                    return Ok(Err(entry));
                }
            }
            _ => {}
        };

        // If we got a move where new from_cs_id is not equal to latest to_cs_id then
        // put it in a separate batch. This shouldn't normally happen though
        if self.to_cs_id != entry.from_changeset_id {
            return Ok(Err(entry));
        }

        self.push(overlay, entry);
        Ok(Ok(()))
    }

    fn push(&mut self, overlay: &mut BookmarkOverlay, entry: BookmarkUpdateLogEntry) {
        self.to_cs_id = entry.to_changeset_id;
        self.entries.push(entry);

        // Side-effect: successfully appending the entry to the batch updates
        // the bookmark value in the overlay.  This means the overlay is
        // correct when computing the server heads for the next batch.
        overlay.update(self.bookmark_name.clone(), self.to_cs_id);
    }
}

async fn split_in_batches(
    ctx: &CoreContext,
    lca_hint: &dyn LeastCommonAncestorsHint,
    changeset_fetcher: &ArcChangesetFetcher,
    overlay: &mut BookmarkOverlay,
    entries: Vec<BookmarkUpdateLogEntry>,
) -> Result<Vec<BookmarkLogEntryBatch>, Error> {
    let mut batches: Vec<BookmarkLogEntryBatch> = vec![];

    for entry in entries {
        let entry = match batches.last_mut() {
            Some(batch) => match batch
                .try_append(ctx, lca_hint, changeset_fetcher, overlay, entry)
                .watched(ctx.logger())
                .await?
            {
                Ok(()) => {
                    continue;
                }
                Err(entry) => entry,
            },
            None => entry,
        };
        batches.push(BookmarkLogEntryBatch::new(overlay, entry));
    }

    Ok(batches)
}

// This function might remove a few first bookmark log entries from the batch.
// It can be useful to modify the very first batch sync job is trying to sync. There can be
// mismatches in what was actually synced to hg servers and the value of "latest-replayed-request"
// counter, and this function can help removing these mismatches.
//
// To be precise:
// 1) If a value of the bookmark in overlay points to from_cs_id in the batch, then batch is not
//    modified. Everything is ok, counter is correct.
// 2) If a value of the bookmark in overlay points to to_changeset_id for the middle bookmark
//    log entry in the batch (say, entry X), then all bookmark log entries up to and including
//    entry X are removed.
//    Usually it means that the everything up to entry X was successfully synced, but sync job failed to
//    update "latest-replayed-request" counter.
// 3) If overlay doesn't point to any commit in the batch, then the batch is not modified.
//    Usually it means that hg server is out of date with hgsql, and we don't need to do anything
impl BookmarkLogEntryBatch {
    pub fn maybe_adjust(&mut self, ctx: &CoreContext) {
        if self.server_old_value == self.from_cs_id {
            // No adjustment needed
            return;
        }

        info!(
            ctx.logger(),
            concat!(
                "trying to adjust first batch for bookmark {} - ",
                "first batch starts points to {:?} but server points to {:?}",
            ),
            self.bookmark_name,
            self.from_cs_id,
            self.server_old_value,
        );

        let mut found = false;
        let mut entries_to_skip = vec![];
        for entry in &self.entries {
            entries_to_skip.push(entry.id);
            if self.server_old_value == entry.to_changeset_id {
                found = true;
                break;
            }
        }

        if found {
            warn!(
                ctx.logger(),
                "adjusting first batch - skipping first entries: {:?}", entries_to_skip
            );
            self.entries.splice(..entries_to_skip.len(), None);
            self.from_cs_id = self.server_old_value;
        } else {
            // The server bookmark doesn't point to any commit from the batch. This might
            // be expected in a repo with high commit rate.
            warn!(
                ctx.logger(),
                concat!(
                    "could not adjust first batch - ",
                    "the server bookmark ({:?}) does not point to any commit in the batch",
                ),
                self.server_old_value
            );
        }
    }
}

#[cfg(test)]
mod test {
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::RepositoryId;
    use skiplist::SkiplistIndex;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::BasicTestRepo;

    use super::*;

    #[fbinit::test]
    async fn test_split_in_batches_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let commit = commits.get("A").cloned().unwrap();
        let entries = vec![create_bookmark_log_entry(
            0,
            main.clone(),
            None,
            Some(commit),
        )];
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let res = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?;

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].entries, entries);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit));

        // The overlay should've been mutated so that the bookmark points to
        // the latest value.
        assert_eq!(overlay.get(&main), Some(commit));

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_in_batches_all_in_one_batch(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let commit_a = commits.get("A").cloned().unwrap();
        let commit_b = commits.get("B").cloned().unwrap();
        let commit_c = commits.get("C").cloned().unwrap();
        let entries = vec![
            create_bookmark_log_entry(0, main.clone(), None, Some(commit_a)),
            create_bookmark_log_entry(1, main.clone(), Some(commit_a), Some(commit_b)),
            create_bookmark_log_entry(2, main.clone(), Some(commit_b), Some(commit_c)),
        ];
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let res = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?;

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].entries, entries);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit_c));

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_in_batches_different_bookmarks(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let another = BookmarkName::new("another")?;
        let commit_a = commits.get("A").cloned().unwrap();
        let commit_b = commits.get("B").cloned().unwrap();
        let commit_c = commits.get("C").cloned().unwrap();
        let log_entry_1 = create_bookmark_log_entry(0, main.clone(), None, Some(commit_a));
        let log_entry_2 = create_bookmark_log_entry(1, another.clone(), None, Some(commit_b));
        let log_entry_3 =
            create_bookmark_log_entry(2, main.clone(), Some(commit_a), Some(commit_c));
        let entries = vec![
            log_entry_1.clone(),
            log_entry_2.clone(),
            log_entry_3.clone(),
        ];
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let res = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?;

        assert_eq!(res.len(), 3);
        assert_eq!(res[0].entries, vec![log_entry_1]);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit_a));

        assert_eq!(res[1].entries, vec![log_entry_2]);
        assert_eq!(res[1].bookmark_name, another);
        assert_eq!(res[1].from_cs_id, None);
        assert_eq!(res[1].to_cs_id, Some(commit_b));

        assert_eq!(res[2].entries, vec![log_entry_3]);
        assert_eq!(res[2].bookmark_name, main);
        assert_eq!(res[2].from_cs_id, Some(commit_a));
        assert_eq!(res[2].to_cs_id, Some(commit_c));

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_in_batches_non_forward_move(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let commit_a = commits.get("A").cloned().unwrap();
        let commit_b = commits.get("B").cloned().unwrap();
        let commit_c = commits.get("C").cloned().unwrap();
        let log_entry_1 = create_bookmark_log_entry(0, main.clone(), None, Some(commit_a));
        let log_entry_2 =
            create_bookmark_log_entry(1, main.clone(), Some(commit_a), Some(commit_c));
        let log_entry_3 =
            create_bookmark_log_entry(2, main.clone(), Some(commit_c), Some(commit_b));
        let entries = vec![
            log_entry_1.clone(),
            log_entry_2.clone(),
            log_entry_3.clone(),
        ];
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let res = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?;

        assert_eq!(res.len(), 2);
        assert_eq!(res[0].entries, vec![log_entry_1, log_entry_2]);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit_c));

        assert_eq!(res[1].entries, vec![log_entry_3]);
        assert_eq!(res[1].bookmark_name, main);
        assert_eq!(res[1].from_cs_id, Some(commit_c));
        assert_eq!(res[1].to_cs_id, Some(commit_b));

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_in_batches_weird_move(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let commit_a = commits.get("A").cloned().unwrap();
        let commit_b = commits.get("B").cloned().unwrap();
        let commit_c = commits.get("C").cloned().unwrap();
        let log_entry_1 = create_bookmark_log_entry(0, main.clone(), None, Some(commit_a));
        let log_entry_2 =
            create_bookmark_log_entry(1, main.clone(), Some(commit_b), Some(commit_c));
        let entries = vec![log_entry_1.clone(), log_entry_2.clone()];
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let res = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?;

        assert_eq!(res.len(), 2);
        assert_eq!(res[0].entries, vec![log_entry_1]);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit_a));

        assert_eq!(res[1].entries, vec![log_entry_2]);
        assert_eq!(res[1].bookmark_name, main);
        assert_eq!(res[1].from_cs_id, Some(commit_b));
        assert_eq!(res[1].to_cs_id, Some(commit_c));

        Ok(())
    }

    #[fbinit::test]
    async fn test_maybe_adjust_batch(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let commits = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C
                D
            "##,
        )
        .await?;

        let sli: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let main = BookmarkName::new("main")?;
        let commit_a = commits.get("A").cloned().unwrap();
        let commit_b = commits.get("B").cloned().unwrap();
        let commit_c = commits.get("C").cloned().unwrap();
        let commit_d = commits.get("D").cloned().unwrap();
        let log_entry_1 = create_bookmark_log_entry(0, main.clone(), None, Some(commit_a));
        let log_entry_2 =
            create_bookmark_log_entry(1, main.clone(), Some(commit_a), Some(commit_b));
        let log_entry_3 =
            create_bookmark_log_entry(1, main.clone(), Some(commit_b), Some(commit_c));
        let entries = vec![log_entry_1, log_entry_2.clone(), log_entry_3.clone()];

        // Default case: no adjustment
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let mut batch = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?
        .into_iter()
        .next()
        .unwrap();
        let original = batch.clone();
        batch.maybe_adjust(&ctx);
        assert_eq!(batch, original);

        // Skip a single entry
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_a,
        }));
        let mut batch = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?
        .into_iter()
        .next()
        .unwrap();
        let original = batch.clone();
        batch.maybe_adjust(&ctx);
        assert_ne!(batch, original);
        assert_eq!(batch.from_cs_id, Some(commit_a));
        assert_eq!(batch.to_cs_id, Some(commit_c));
        assert_eq!(batch.entries, vec![log_entry_2, log_entry_3.clone()]);

        // Skip two entries
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_b,
        }));
        let mut batch = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?
        .into_iter()
        .next()
        .unwrap();
        let original = batch.clone();
        batch.maybe_adjust(&ctx);
        assert_ne!(batch, original);
        assert_eq!(batch.from_cs_id, Some(commit_b));
        assert_eq!(batch.to_cs_id, Some(commit_c));
        assert_eq!(batch.entries, vec![log_entry_3]);

        // The whole batch was already synced - nothing to do!
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_c,
        }));
        let mut batch = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?
        .into_iter()
        .next()
        .unwrap();
        let original = batch.clone();
        batch.maybe_adjust(&ctx);
        assert_ne!(batch, original);
        assert!(batch.is_empty());

        // Bookmark is not in the batch at all - in that case just do nothing and
        // return existing bundle
        let mut overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main => commit_d,
        }));
        let mut batch = split_in_batches(
            &ctx,
            &sli,
            &repo.changeset_fetcher_arc(),
            &mut overlay,
            entries.clone(),
        )
        .await?
        .into_iter()
        .next()
        .unwrap();
        let original = batch.clone();
        batch.maybe_adjust(&ctx);
        assert_eq!(batch, original);
        Ok(())
    }

    fn create_bookmark_log_entry(
        id: i64,
        bookmark_name: BookmarkName,
        from_changeset_id: Option<ChangesetId>,
        to_changeset_id: Option<ChangesetId>,
    ) -> BookmarkUpdateLogEntry {
        BookmarkUpdateLogEntry {
            id,
            repo_id: RepositoryId::new(0),
            bookmark_name,
            from_changeset_id,
            to_changeset_id,
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }
    }
}
