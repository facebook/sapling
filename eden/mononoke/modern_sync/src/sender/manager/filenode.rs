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
use mercurial_types::HgFileNodeId;
use metaconfig_types::ModernSyncChannelConfig;
use mononoke_macros::mononoke;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::FileMessage;
use crate::sender::manager::Manager;

define_stats! {
    prefix = "mononoke.modern_sync.manager.filenode";

    synced_filenodes:  dynamic_timeseries("{}.synced_filenodes", (repo: String); Sum),
    content_wait_time_s:  dynamic_timeseries("{}.content_wait_time_s", (repo: String); Average),

    files_queue_capacity: dynamic_singleton_counter("{}.files.queue_capacity", (repo: String)),
    files_queue_len: dynamic_histogram("{}.files.queue_len", (repo: String); 10, 0, 100_000, Average; P 50; P 75; P 95; P 99),
    files_queue_max_capacity: dynamic_singleton_counter("{}.files.queue_max_capacity", (repo: String)),
}

pub(crate) struct FilenodeManager {
    config: ModernSyncChannelConfig,
    filenodes_recv: mpsc::Receiver<FileMessage>,
}

impl FilenodeManager {
    pub(crate) fn new(
        config: ModernSyncChannelConfig,
        filenodes_recv: mpsc::Receiver<FileMessage>,
    ) -> Self {
        Self {
            config,
            filenodes_recv,
        }
    }

    async fn flush_filenodes(
        filenodes_es: &Arc<dyn EdenapiSender + Send + Sync>,
        batch_filenodes: &mut Vec<HgFileNodeId>,
        batch_done_senders: &mut VecDeque<oneshot::Sender<Result<()>>>,
        encountered_error: &mut Option<anyhow::Error>,
        reponame: &str,
    ) -> Result<(), anyhow::Error> {
        if !batch_filenodes.is_empty() || !batch_done_senders.is_empty() {
            let batch_size = batch_filenodes.len() as i64;
            if let Some(e) = encountered_error {
                let msg = format!("Error processing filenodes: {:?}", e);
                while let Some(sender) = batch_done_senders.pop_front() {
                    let _ = sender.send(Err(anyhow::anyhow!(msg.clone())));
                }
                tracing::error!("Error processing filenodes: {:?}", e);
                return Err(anyhow::anyhow!(msg.clone()));
            }

            if !batch_filenodes.is_empty() {
                let start = std::time::Instant::now();
                if let Err(e) = filenodes_es
                    .upload_filenodes(std::mem::take(batch_filenodes))
                    .await
                {
                    tracing::error!("Failed to upload filenodes: {:?}", e);
                    return Err(e);
                } else {
                    tracing::info!(
                        "Uploaded {} filenodes in {}ms",
                        batch_size,
                        start.elapsed().as_millis(),
                    );
                    STATS::synced_filenodes
                        .add_value(batch_filenodes.len() as i64, (reponame.to_owned(),));
                }
            }

            while let Some(sender) = batch_done_senders.pop_front() {
                let res = sender.send(Ok(()));
                if let Err(e) = res {
                    let msg = format!("Error sending filenodes ready: {:?}", e);
                    tracing::error!("{}", msg);
                    return Err(anyhow::anyhow!(msg));
                }
            }
        }
        Ok(())
    }
}

impl Manager for FilenodeManager {
    fn start(
        mut self,
        ctx: CoreContext,
        reponame: String,
        filenodes_es: Arc<dyn EdenapiSender + Send + Sync>,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let filenodes_recv = &mut self.filenodes_recv;

            let mut encountered_error: Option<anyhow::Error> = None;
            let mut batch_filenodes = Vec::new();
            let mut batch_done_senders = VecDeque::new();
            let mut timer = interval(Duration::from_millis(self.config.flush_interval_ms as u64));

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = filenodes_recv.recv() => {
                        tracing::debug!("Filenodes channel capacity: {} max capacity: {} in queue: {}", filenodes_recv.capacity(), self.config.channel_size,  filenodes_recv.len());
                        STATS::files_queue_capacity.set_value(ctx.fb, filenodes_recv.capacity() as i64, (reponame.clone(),));
                        STATS::files_queue_len.add_value(filenodes_recv.len() as i64, (reponame.clone(),));
                        STATS::files_queue_max_capacity.set_value(ctx.fb, filenodes_recv.max_capacity() as i64, (reponame.clone(),));
                        match msg {
                            Some(FileMessage::WaitForContents(receiver)) => {
                                let start = std::time::Instant::now();
                                match receiver.await {
                                    Ok(Err(e)) => {
                                        encountered_error.get_or_insert(e.context(
                                            "Contents error received. Winding down files sender."
                                        ));
                                    }
                                    _ => (),
                                }
                                let elapsed = start.elapsed().as_secs();
                                STATS::content_wait_time_s.add_value(elapsed as i64, (reponame.clone(),));
                            }
                            Some(FileMessage::FileNode(f)) if encountered_error.is_none() => {
                                batch_filenodes.push(f);
                            }
                            Some(FileMessage::FilesDone(sender)) => {
                                batch_done_senders.push_back(sender);
                            }
                            Some(FileMessage::FileNode(_)) => (),
                            None => break,
                        }
                        if batch_filenodes.len() >= self.config.batch_size as usize {
                            if let Err(e) = FilenodeManager::flush_filenodes(&filenodes_es, &mut batch_filenodes, &mut batch_done_senders, &mut encountered_error, &reponame).await {
                                tracing::error!("Filenodes flush failed: {:?}", e);
                                return;
                            }
                        }
                    }
                    _ = timer.tick() => {
                        if let Err(e) = FilenodeManager::flush_filenodes(&filenodes_es, &mut batch_filenodes, &mut batch_done_senders, &mut encountered_error, &reponame).await {
                            tracing::error!("Filenodes flush failed: {:?}", e);
                            return;
                        }
                    }
                }
            }
        });
    }
}
