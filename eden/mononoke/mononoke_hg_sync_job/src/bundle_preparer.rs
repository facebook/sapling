/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
use anyhow::anyhow;
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
use metaconfig_types::LfsParams;
use mononoke_hg_sync_job_helper_lib::save_bytes_to_temp_file;
use mononoke_hg_sync_job_helper_lib::write_to_named_temp_file;
use mononoke_types::datetime::Timestamp;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use regex::Regex;
use slog::info;
use slog::warn;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::NamedTempFile;

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
    bundle_info: BundleInfo,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
}

#[derive(Clone)]
struct PrepareInfo {
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    lfs_params: SessionLfsParams,
    filenode_verifier: FilenodeVerifier,
}

#[derive(Clone)]
struct BundleInfo {
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    lfs_params: LfsParams,
    filenode_verifier: FilenodeVerifier,
    bookmark_regex_force_lfs: Option<Regex>,
    use_hg_server_bookmark_value_if_mismatch: bool,
}

impl BundlePreparer {
    pub async fn new_generate_bundles(
        repo: Repo,
        base_retry_delay_ms: u64,
        retry_num: usize,
        lfs_params: LfsParams,
        filenode_verifier: FilenodeVerifier,
        bookmark_regex_force_lfs: Option<Regex>,
        use_hg_server_bookmark_value_if_mismatch: bool,
        push_vars: Option<HashMap<String, bytes::Bytes>>,
    ) -> Result<BundlePreparer, Error> {
        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = repo.skiplist_index.clone();
        Ok(BundlePreparer {
            repo,
            base_retry_delay_ms,
            retry_num,
            bundle_info: BundleInfo {
                lca_hint,
                lfs_params,
                filenode_verifier,
                bookmark_regex_force_lfs,
                use_hg_server_bookmark_value_if_mismatch,
            },
            push_vars,
        })
    }

    pub async fn prepare_batches(
        &self,
        ctx: &CoreContext,
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
            &self.bundle_info.lca_hint,
            &self.repo.changeset_fetcher_arc(),
            entries,
        )
        .await
    }

    pub fn prepare_bundles(
        &self,
        ctx: CoreContext,
        batches: Vec<BookmarkLogEntryBatch>,
        overlay: &mut crate::BookmarkOverlay,
    ) -> BoxFuture<'static, Result<Vec<CombinedBookmarkUpdateLogEntry>, PipelineError>> {
        let mut futs = vec![];
        let push_vars = self.push_vars.clone();

        let BundleInfo {
            lca_hint,
            lfs_params,
            filenode_verifier,
            bookmark_regex_force_lfs,
            use_hg_server_bookmark_value_if_mismatch,
        } = &self.bundle_info;
        for batch in batches {
            let prepare_type = PrepareInfo {
                lca_hint: lca_hint.clone(),
                lfs_params: get_session_lfs_params(
                    &ctx,
                    &batch.bookmark_name,
                    lfs_params.clone(),
                    bookmark_regex_force_lfs,
                ),
                filenode_verifier: filenode_verifier.clone(),
            };

            let entries = batch.entries.clone();
            let f = self.prepare_single_bundle(
                ctx.clone(),
                batch,
                overlay,
                prepare_type,
                *use_hg_server_bookmark_value_if_mismatch,
                push_vars.clone(),
            );
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
        mut batch: BookmarkLogEntryBatch,
        overlay: &mut crate::BookmarkOverlay,
        prepare_info: PrepareInfo,
        use_hg_server_bookmark_value_if_mismatch: bool,
        push_vars: Option<HashMap<String, bytes::Bytes>>,
    ) -> BoxFuture<'static, Result<CombinedBookmarkUpdateLogEntry, Error>> {
        cloned!(self.repo);

        if use_hg_server_bookmark_value_if_mismatch {
            if !overlay.is_in_overlay(&batch.bookmark_name) {
                // If it's not in overlay then it came from hg server.
                // In that case compare if the value from hg server match
                // whatever we have in the bookmark log entry batch.
                let overlay_bookmark_value = overlay.get_value(&batch.bookmark_name);
                if overlay_bookmark_value != batch.from_cs_id {
                    warn!(
                        ctx.logger(),
                        "{} is expected to point to {:?}, but it actually points to {:?} on hg server. \
                        Forcing {} to point to {:?}",
                        batch.bookmark_name,
                        batch.from_cs_id,
                        overlay_bookmark_value,
                        batch.bookmark_name,
                        overlay_bookmark_value,
                    );
                    batch.from_cs_id = overlay_bookmark_value;
                }
            }
        }

        let book_values = overlay.get_bookmark_values();
        overlay.update(batch.bookmark_name.clone(), batch.to_cs_id.clone());

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
            let bundle_timestamps_commits = retry::retry(
                ctx.logger(),
                {
                    |_| {
                        Self::try_prepare_bundle_timestamps_file(
                            &ctx,
                            &repo,
                            prepare_info.clone(),
                            &book_values,
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

    async fn try_prepare_bundle_timestamps_file<'a>(
        ctx: &'a CoreContext,
        repo: &'a Repo,
        prepare_info: PrepareInfo,
        hg_server_heads: &'a [ChangesetId],
        bookmark_change: &'a BookmarkChange,
        bookmark_name: &'a BookmarkName,
        push_vars: Option<HashMap<String, bytes::Bytes>>,
    ) -> Result<(NamedTempFile, NamedTempFile, CommitsInBundle), Error> {
        let PrepareInfo {
            lca_hint,
            lfs_params,
            filenode_verifier,
        } = prepare_info;
        let (bytes, timestamps) = crate::bundle_generator::create_bundle(
            ctx.clone(),
            repo.clone(),
            lca_hint.clone(),
            bookmark_name.clone(),
            bookmark_change.clone(),
            hg_server_heads.to_vec(),
            lfs_params,
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
}

fn get_session_lfs_params(
    ctx: &CoreContext,
    bookmark: &BookmarkName,
    lfs_params: LfsParams,
    bookmark_regex_force_lfs: &Option<Regex>,
) -> SessionLfsParams {
    if let Some(regex) = bookmark_regex_force_lfs {
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
}

impl BookmarkLogEntryBatch {
    pub fn new(log_entry: BookmarkUpdateLogEntry) -> Self {
        let bookmark_name = log_entry.bookmark_name.clone();
        let from_cs_id = log_entry.from_changeset_id;
        let to_cs_id = log_entry.to_changeset_id;
        Self {
            entries: vec![log_entry],
            bookmark_name,
            from_cs_id,
            to_cs_id,
        }
    }

    // Outer result's error means that some infrastructure error happened.
    // Inner result's error means that it wasn't possible to append entry,
    // and this entry is returned as the error.
    pub async fn try_append(
        &mut self,
        ctx: &CoreContext,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        changeset_fetcher: &ArcChangesetFetcher,
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

        self.push(entry);
        Ok(Ok(()))
    }

    fn push(&mut self, entry: BookmarkUpdateLogEntry) {
        self.to_cs_id = entry.to_changeset_id;
        self.entries.push(entry);
    }

    pub fn remove_first_entries(
        mut self,
        num_entries_to_remove: usize,
    ) -> Result<Option<Self>, Error> {
        if num_entries_to_remove > self.entries.len() {
            return Err(anyhow!(
                "Programmer error: tried to skip more entries that the batch has"
            ));
        }

        let last_entries = self.entries.split_off(num_entries_to_remove);
        self.entries = last_entries;
        if let Some(entry) = self.entries.get(0) {
            self.from_cs_id = entry.from_changeset_id;
            Ok(Some(self))
        } else {
            Ok(None)
        }
    }
}

async fn split_in_batches(
    ctx: &CoreContext,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    changeset_fetcher: &ArcChangesetFetcher,
    entries: Vec<BookmarkUpdateLogEntry>,
) -> Result<Vec<BookmarkLogEntryBatch>, Error> {
    let mut batches: Vec<BookmarkLogEntryBatch> = vec![];

    for entry in entries {
        let entry = match batches.last_mut() {
            Some(batch) => match batch
                .try_append(ctx, lca_hint, changeset_fetcher, entry)
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
        batches.push(BookmarkLogEntryBatch::new(entry));
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
pub fn maybe_adjust_batch(
    ctx: &CoreContext,
    batch: BookmarkLogEntryBatch,
    overlay: &BookmarkOverlay,
) -> Result<Option<BookmarkLogEntryBatch>, Error> {
    let book_name = &batch.bookmark_name;

    // No adjustment needed
    let overlay_value = overlay.get_value(book_name);
    if overlay_value == batch.from_cs_id {
        return Ok(Some(batch));
    }

    info!(
        ctx.logger(),
        "trying to adjust first batch for bookmark {}. \
        First batch starts points to {:?}",
        book_name,
        batch.from_cs_id
    );

    let mut found = false;
    let mut entries_to_skip = vec![];
    for entry in &batch.entries {
        entries_to_skip.push(entry.id);
        if overlay_value == entry.to_changeset_id {
            found = true;
            break;
        }
    }

    if found {
        warn!(
            ctx.logger(),
            "adjusting first batch - skipping first entries {:?}", entries_to_skip
        );
        batch.remove_first_entries(entries_to_skip.len())
    } else {
        warn!(
            ctx.logger(),
            "could not adjust first batch, because bookmark hg \
            server bookmark doesn't point to any commit from the batch. This might \
            be expected in a repo with high commit rate in case hg server is out of \
            sync with hgsql."
        );
        Ok(Some(batch))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::RepositoryId;
    use skiplist::SkiplistIndex;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::BasicTestRepo;

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
        let res =
            split_in_batches(&ctx, &sli, &repo.changeset_fetcher_arc(), entries.clone()).await?;

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].entries, entries);
        assert_eq!(res[0].bookmark_name, main);
        assert_eq!(res[0].from_cs_id, None);
        assert_eq!(res[0].to_cs_id, Some(commit));

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
        let res =
            split_in_batches(&ctx, &sli, &repo.changeset_fetcher_arc(), entries.clone()).await?;

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
        let res =
            split_in_batches(&ctx, &sli, &repo.changeset_fetcher_arc(), entries.clone()).await?;

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
        let res =
            split_in_batches(&ctx, &sli, &repo.changeset_fetcher_arc(), entries.clone()).await?;

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
        let res =
            split_in_batches(&ctx, &sli, &repo.changeset_fetcher_arc(), entries.clone()).await?;

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

        let mut batch = BookmarkLogEntryBatch::new(log_entry_1);
        batch.push(log_entry_2.clone());
        batch.push(log_entry_3.clone());

        let overlay = BookmarkOverlay::new(Arc::new(hashmap! {}));
        let adjusted = maybe_adjust_batch(&ctx, batch.clone(), &overlay)?;
        assert_eq!(Some(batch.clone()), adjusted);

        // Skip a single entry
        let overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_a,
        }));
        let adjusted = maybe_adjust_batch(&ctx, batch.clone(), &overlay)?;
        assert!(adjusted.is_some());
        assert_ne!(Some(batch.clone()), adjusted);
        let adjusted = adjusted.unwrap();
        assert_eq!(adjusted.from_cs_id, Some(commit_a));
        assert_eq!(adjusted.to_cs_id, Some(commit_c));
        assert_eq!(adjusted.entries, vec![log_entry_2, log_entry_3.clone()]);

        // Skip two entries
        let overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_b,
        }));
        let adjusted = maybe_adjust_batch(&ctx, batch.clone(), &overlay)?;
        assert!(adjusted.is_some());
        assert_ne!(Some(batch.clone()), adjusted);
        let adjusted = adjusted.unwrap();
        assert_eq!(adjusted.from_cs_id, Some(commit_b));
        assert_eq!(adjusted.to_cs_id, Some(commit_c));
        assert_eq!(adjusted.entries, vec![log_entry_3]);

        // The whole batch was already synced - nothing to do!
        let overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main.clone() => commit_c,
        }));
        let adjusted = maybe_adjust_batch(&ctx, batch.clone(), &overlay)?;
        assert_eq!(None, adjusted);

        // Bookmark is not in the batch at all - in that case just do nothing and
        // return existing bundle
        let overlay = BookmarkOverlay::new(Arc::new(hashmap! {
          main => commit_d,
        }));
        let adjusted = maybe_adjust_batch(&ctx, batch.clone(), &overlay)?;
        assert_eq!(Some(batch), adjusted);
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
