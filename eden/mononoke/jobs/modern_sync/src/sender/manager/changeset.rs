/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use context::CoreContext;
use futures::channel::oneshot;
use mercurial_types::blobs::HgBlobChangeset;
use metaconfig_types::ModernSyncChannelConfig;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mutable_counters::MutableCounters;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::BookmarkInfo;
use crate::sender::manager::ChangesetMessage;
use crate::sender::manager::MODERN_SYNC_BATCH_CHECKPOINT_NAME;
use crate::sender::manager::MODERN_SYNC_COUNTER_NAME;
use crate::sender::manager::MODERN_SYNC_CURRENT_ENTRY_ID;
use crate::sender::manager::Manager;
use crate::stat;

define_stats! {
    prefix = "mononoke.modern_sync.manager.changeset";

    synced_commits:  dynamic_timeseries("{}.commits_synced", (repo: String); Sum),
    sync_lag_seconds:  dynamic_timeseries("{}.sync_lag_seconds", (repo: String); Average),
    trees_files_wait_time_s:  dynamic_timeseries("{}.trees_files_wait_time_s", (repo: String); Average),
    changeset_upload_time_s:  dynamic_timeseries("{}.changeset_upload_time_s", (repo: String); Average),

    changesets_queue_capacity: dynamic_singleton_counter("{}.changesets.queue_capacity", (repo: String)),
    changesets_queue_len: dynamic_histogram("{}.changesets.queue_len", (repo: String); 10, 0, 100_000, Average; P 50; P 75; P 95; P 99),
    changesets_queue_max_capacity: dynamic_singleton_counter("{}.changesets.queue_max_capacity", (repo: String)),
}

pub(crate) struct ChangesetManager {
    config: ModernSyncChannelConfig,
    changeset_recv: mpsc::Receiver<ChangesetMessage>,
    mc: Arc<dyn MutableCounters + Send + Sync>,
}

impl ChangesetManager {
    pub(crate) fn new(
        config: ModernSyncChannelConfig,
        changeset_recv: mpsc::Receiver<ChangesetMessage>,
        mc: Arc<dyn MutableCounters + Send + Sync>,
    ) -> Self {
        Self {
            config,
            changeset_recv,
            mc,
        }
    }

    async fn flush_batch(
        reponame: String,
        ctx: &CoreContext,
        changeset_es: &Arc<dyn EdenapiSender + Send + Sync>,
        mc: Arc<dyn MutableCounters + Send + Sync>,
        current_batch: &mut Vec<(HgBlobChangeset, BonsaiChangeset)>,
        pending_log: &mut VecDeque<Option<i64>>,
        latest_checkpoint: &mut Option<(u64, i64)>,
        latest_entry_id: &mut Option<i64>,
        latest_bookmark: &mut Option<BookmarkInfo>,
        pending_notification: &mut Option<oneshot::Sender<Result<()>>>,
    ) -> Result<(), anyhow::Error> {
        if !current_batch.is_empty() {
            let start = std::time::Instant::now();
            let batch_size = current_batch.len();
            if let Err(e) = changeset_es
                .upload_identical_changeset(std::mem::take(current_batch))
                .await
            {
                tracing::error!("Failed to upload changesets {:?} {:?}", current_batch, e);
                return Err(e);
            } else {
                let elapsed = start.elapsed().as_secs() / batch_size as u64;
                STATS::changeset_upload_time_s.add_value(elapsed as i64, (reponame.clone(),));
                STATS::synced_commits.add_value(batch_size as i64, (reponame.clone(),));
            }
        }

        while let Some(Some(lag)) = pending_log.pop_front() {
            STATS::sync_lag_seconds.add_value(lag, (reponame.clone(),));
        }

        if let Some((position, id)) = latest_checkpoint.take() {
            tracing::info!("Setting checkpoint from entry {} to {}", id, position);

            let res_entry = mc
                .set_counter(ctx, MODERN_SYNC_CURRENT_ENTRY_ID, id, None)
                .await?;

            let res_checkpoint = mc
                .set_counter(
                    ctx,
                    MODERN_SYNC_BATCH_CHECKPOINT_NAME,
                    position.try_into().unwrap(),
                    None,
                )
                .await?;

            if !(res_checkpoint && res_entry) {
                tracing::warn!(
                    "Failed to checkpoint entry {} at position {:?}",
                    id,
                    position
                );
            }
        }

        if let Some(info) = latest_bookmark.take() {
            tracing::info!(
                "Setting bookmark {} from {:?} to {:?}",
                info.name,
                info.from_cs_id,
                info.to_cs_id
            );
            changeset_es
                .set_bookmark(info.name, info.from_cs_id, info.to_cs_id)
                .await?;
        }

        if let Some(id) = latest_entry_id.take() {
            tracing::info!("Marking entry {} as done", id);
            let res = mc
                .set_counter(ctx, MODERN_SYNC_COUNTER_NAME, id, None)
                .await?;

            if !res {
                tracing::warn!("Failed to mark entry {} as synced", id);
            }
        }

        if let Some(sender) = pending_notification.take() {
            let _ = sender.send(Ok(()));
        }

        Ok(())
    }
}

impl Manager for ChangesetManager {
    fn start(
        mut self,
        ctx: CoreContext,
        reponame: String,
        changeset_es: Arc<dyn EdenapiSender + Send + Sync>,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let changeset_recv = &mut self.changeset_recv;
            let mc = &self.mc;

            let mut encountered_error: Option<anyhow::Error> = None;

            let mut pending_log = VecDeque::new();

            let mut latest_in_entry_checkpoint = None;
            let mut latest_entry_id = None;
            let mut latest_bookmark: Option<BookmarkInfo> = None;
            let mut pending_notification = None;

            let mut current_batch = Vec::new();
            let mut flush_timer =
                interval(Duration::from_millis(self.config.flush_interval_ms as u64));

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {

                    msg = changeset_recv.recv() => {

                        tracing::debug!(
                            "Changeset channel capacity: {} max capacity: {} in queue: {}",
                            changeset_recv.capacity(),
                            self.config.channel_size,
                            changeset_recv.len()
                        );
                        STATS::changesets_queue_capacity.set_value(ctx.fb, changeset_recv.capacity() as i64, (reponame.clone(),));
                        STATS::changesets_queue_len.add_value(changeset_recv.len() as i64, (reponame.clone(),));
                        STATS::changesets_queue_max_capacity.set_value(ctx.fb, changeset_recv.max_capacity() as i64, (reponame.clone(),));
                        match msg {
                            Some(ChangesetMessage::WaitForFilesAndTrees(
                                files_receiver,
                                trees_receiver,
                            )) => {
                                // Read outcome from files and trees upload
                                let start = std::time::Instant::now();
                                match tokio::try_join!(files_receiver, trees_receiver) {
                                    Ok((res_files, res_trees)) => {
                                        if res_files.is_err() || res_trees.is_err() {
                                            tracing::error!(
                                                "Error processing files/trees: {:?} {:?}",
                                                res_files,
                                                res_trees
                                            );
                                            encountered_error.get_or_insert(anyhow::anyhow!(
                                                "Files/trees error received. Winding down changesets sender.",
                                            ));
                                        }
                                        let elapsed = start.elapsed().as_secs();
                                        STATS::trees_files_wait_time_s
                                            .add_value(elapsed as i64, (reponame.clone(),));
                                    }
                                    Err(e) => {
                                        encountered_error.get_or_insert(anyhow::anyhow!(
                                            "Error waiting for files/trees error received {:#}",
                                            e
                                        ));
                                    }
                                }
                            }

                            Some(ChangesetMessage::Changeset((hg_cs, bcs)))
                                if encountered_error.is_none() =>
                            {
                                current_batch.push((hg_cs, bcs));
                            }

                            Some(ChangesetMessage::CheckpointInEntry(position, id))
                                if encountered_error.is_none() =>
                            {
                                latest_in_entry_checkpoint = Some((position, id));
                            }

                            Some(ChangesetMessage::FinishEntry(bookmark, id))
                                if encountered_error.is_none() =>
                            {
                                latest_entry_id = Some(id);
                                if let Some(prev_bookmark) = latest_bookmark {
                                    latest_bookmark = Some(BookmarkInfo {
                                        name: prev_bookmark.name,
                                        from_cs_id: prev_bookmark.from_cs_id,
                                        to_cs_id: bookmark.to_cs_id,
                                    });
                                } else {
                                    latest_bookmark = Some(bookmark);
                                }
                            }

                            Some(ChangesetMessage::NotifyCompletion(sender))
                                if encountered_error.is_none() =>
                            {
                                pending_notification = Some(sender);
                            }

                            Some(ChangesetMessage::NotifyCompletion(sender)) => {
                                let e = encountered_error.unwrap();
                                let _ = sender.send(Err(anyhow::anyhow!(
                                    "Error processing changesets: {:?}",
                                    e
                                )));
                                return Err(e);
                            }

                            Some(ChangesetMessage::Log((_, lag)))
                                if encountered_error.is_none() =>
                            {
                                pending_log.push_back(lag);
                            }
                            None => break,

                            // Ignore any other action if there's an error
                            _ => {}
                        }

                        if current_batch.len() >= self.config.batch_size as usize {
                            let now = std::time::Instant::now();
                            let changeset_ids = current_batch.iter().map(|c| c.1.get_changeset_id()).collect::<Vec<_>>();
                            stat::log_upload_changeset_start(&ctx, changeset_ids.clone());
                            match ChangesetManager::flush_batch(reponame.clone(), &ctx, &changeset_es, mc.clone(), &mut current_batch, &mut pending_log, &mut latest_in_entry_checkpoint, &mut latest_entry_id, &mut latest_bookmark, &mut pending_notification)
                            .await
                            {
                                Ok(()) => {
                                    stat::log_upload_changeset_done(&ctx, changeset_ids, now.elapsed());
                                }
                                Err(e) => {
                                    stat::log_upload_changeset_error(&ctx, changeset_ids, &e, now.elapsed());
                                    return Err(anyhow::anyhow!(
                                        "Error processing changesets: {:?}",
                                        e
                                    ));
                                }
                            }
                        }
                    }
                    _ = flush_timer.tick() =>
                    {
                        let now = std::time::Instant::now();
                        let changeset_ids = current_batch.iter().map(|c| c.1.get_changeset_id()).collect::<Vec<_>>();
                        stat::log_upload_changeset_start(&ctx, changeset_ids.clone());
                        match ChangesetManager::flush_batch(reponame.clone(), &ctx, &changeset_es, mc.clone(), &mut current_batch, &mut pending_log, &mut latest_in_entry_checkpoint, &mut latest_entry_id, &mut latest_bookmark, &mut pending_notification)
                        .await
                        {
                            Ok(()) => {
                                    stat::log_upload_changeset_done(&ctx, changeset_ids, now.elapsed());
                            }
                            Err(e) => {
                                stat::log_upload_changeset_error(&ctx, changeset_ids, &e, now.elapsed());
                                return Err(anyhow::anyhow!("Error processing changesets: {:?}", e));
                            }
                        }
                    }
                }
            }

            Ok(())
        });
    }
}
