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
use mercurial_types::HgManifestId;
use metaconfig_types::ModernSyncChannelConfig;
use mononoke_macros::mononoke;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::Manager;
use crate::sender::manager::TreeMessage;

define_stats! {
    prefix = "mononoke.modern_sync.manager.tree";

    synced_trees:  dynamic_timeseries("{}.synced_trees", (repo: String); Sum),
    content_wait_time_s:  dynamic_timeseries("{}.content_wait_time_s", (repo: String); Average),

    trees_queue_capacity: dynamic_singleton_counter("{}.trees.queue_capacity", (repo: String)),
    trees_queue_len: dynamic_histogram("{}.trees.queue_len", (repo: String); 10, 0,  100_000, Average; P 50; P 75; P 95; P 99),
    trees_queue_max_capacity: dynamic_singleton_counter("{}.trees.queue_max_capacity", (repo: String)),
}

pub(crate) struct TreeManager {
    config: ModernSyncChannelConfig,
    trees_recv: mpsc::Receiver<TreeMessage>,
}

impl TreeManager {
    pub(crate) fn new(
        config: ModernSyncChannelConfig,
        trees_recv: mpsc::Receiver<TreeMessage>,
    ) -> Self {
        Self { config, trees_recv }
    }

    async fn flush_trees(
        trees_es: &Arc<dyn EdenapiSender + Send + Sync>,
        batch_trees: &mut Vec<HgManifestId>,
        batch_done_senders: &mut VecDeque<oneshot::Sender<Result<()>>>,
        encountered_error: &mut Option<anyhow::Error>,
        reponame: &str,
    ) -> Result<(), anyhow::Error> {
        if !batch_trees.is_empty() || !batch_done_senders.is_empty() {
            let batch_size = batch_trees.len() as i64;
            if let Some(e) = encountered_error {
                let msg = format!("Error processing trees: {:?}", e);
                while let Some(sender) = batch_done_senders.pop_front() {
                    let _ = sender.send(Err(anyhow::anyhow!(msg.clone())));
                }
                tracing::error!("Error processing files/trees: {:?}", e);
                return Err(anyhow::anyhow!(msg.clone()));
            }

            if !batch_trees.is_empty() {
                let start = std::time::Instant::now();
                if let Err(e) = trees_es.upload_trees(std::mem::take(batch_trees)).await {
                    tracing::error!("Failed to upload trees: {:?}", e);
                    return Err(e);
                } else {
                    tracing::info!(
                        "Uploaded {} trees in {}ms",
                        batch_size,
                        start.elapsed().as_millis(),
                    );
                    STATS::synced_trees.add_value(batch_size, (reponame.to_owned(),));
                }
            }

            while let Some(sender) = batch_done_senders.pop_front() {
                let res = sender.send(Ok(()));
                if let Err(e) = res {
                    let msg = format!("Error sending content ready: {:?}", e);
                    tracing::error!("{}", msg);
                    return Err(anyhow::anyhow!(msg));
                }
            }
        }
        Ok(())
    }
}

impl Manager for TreeManager {
    fn start(
        mut self,
        ctx: CoreContext,
        reponame: String,
        trees_es: Arc<dyn EdenapiSender + Send + Sync>,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let trees_recv = &mut self.trees_recv;

            let mut encountered_error: Option<anyhow::Error> = None;
            let mut batch_trees = Vec::new();
            let mut batch_done_senders = VecDeque::new();
            let mut timer = interval(Duration::from_millis(self.config.flush_interval_ms as u64));

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = trees_recv.recv() => {
                        tracing::debug!("Trees channel capacity: {} max capacity: {} in queue: {}", trees_recv.capacity(), self.config.channel_size,  trees_recv.len());
                        STATS::trees_queue_capacity.set_value(ctx.fb, trees_recv.capacity() as i64, (reponame.clone(),));
                        STATS::trees_queue_len.add_value(trees_recv.len() as i64, (reponame.clone(),));
                        STATS::trees_queue_max_capacity.set_value(ctx.fb, trees_recv.max_capacity() as i64, (reponame.clone(),));
                        match msg {
                            Some(TreeMessage::WaitForContents(receiver)) => {
                                // Read outcome from content upload
                                let start = std::time::Instant::now();
                                match receiver.await {
                                    Ok(Err(e)) => {
                                        encountered_error.get_or_insert(e.context(
                                            "Contents error received. Winding down trees sender.",
                                        ));
                                    }
                                    Err(e) => {
                                        encountered_error.get_or_insert(anyhow::anyhow!(format!(
                                            "Error waiting for contents: {:#}",
                                            e
                                        )));
                                    }
                                    _ => (),
                                }
                                let elapsed = start.elapsed().as_secs();
                                STATS::content_wait_time_s.add_value(elapsed as i64, (reponame.clone(),));
                            }
                            Some(TreeMessage::Tree(t)) if encountered_error.is_none() => {
                                batch_trees.push(t);
                            }
                            Some(TreeMessage::TreesDone(sender)) => {
                                batch_done_senders.push_back(sender);
                            }
                            Some(TreeMessage::Tree(_)) => (),
                            None => break,
                        }
                        if batch_trees.len() >= self.config.batch_size as usize {
                            if let Err(e) = TreeManager::flush_trees(&trees_es, &mut batch_trees, &mut batch_done_senders, &mut encountered_error, &reponame).await {
                                tracing::error!("Trees flush failed: {:?}", e);
                                return;
                            }
                        }
                    }
                    _ = timer.tick() => {
                        if let Err(e) = TreeManager::flush_trees(&trees_es, &mut batch_trees, &mut batch_done_senders, &mut encountered_error, &reponame).await {
                            tracing::error!("Trees flush failed: {:?}", e);
                            return;
                        }
                    }
                }
            }
        });
    }
}
