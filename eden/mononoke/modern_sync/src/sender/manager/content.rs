/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use bytesize::ByteSize;
use context::CoreContext;
use futures::channel::oneshot;
use mononoke_macros::mononoke;
use mononoke_types::ContentId;
use slog::debug;
use slog::error;
use slog::info;
use slog::Logger;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::ContentMessage;
use crate::sender::manager::Manager;
use crate::sender::manager::CONTENTS_FLUSH_INTERVAL;
use crate::sender::manager::CONTENT_CHANNEL_SIZE;
use crate::sender::manager::MAX_BLOB_BYTES;
use crate::sender::manager::MAX_CONTENT_BATCH_SIZE;

define_stats! {
    prefix = "mononoke.modern_sync.manager.content";

    synced_contents:  dynamic_timeseries("{}.synced_contents", (repo: String); Sum),
    content_upload_time_s:  dynamic_timeseries("{}.content_upload_time_ms", (repo: String); Average),

    contents_queue_capacity: dynamic_singleton_counter("{}.contents.queue_capacity", (repo: String)),
    contents_queue_len: dynamic_histogram("{}.contents.queue_len", (repo: String); 10, 0, crate::sender::manager::CONTENT_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    contents_queue_max_capacity: dynamic_singleton_counter("{}.contents.queue_max_capacity", (repo: String)),
}

pub(crate) struct ContentManager {
    content_recv: mpsc::Receiver<ContentMessage>,
}

impl ContentManager {
    pub(crate) fn new(content_recv: mpsc::Receiver<ContentMessage>) -> Self {
        Self { content_recv }
    }

    async fn flush_batch(
        content_es: &Arc<dyn EdenapiSender + Send + Sync>,
        current_batch: &mut Vec<ContentId>,
        current_batch_size: u64,
        pending_messages: &mut VecDeque<oneshot::Sender<Result<(), anyhow::Error>>>,
        reponame: String,
        logger: &Logger,
    ) -> Result<(), anyhow::Error> {
        let current_batch_len = current_batch.len() as i64;
        let start = std::time::Instant::now();
        if current_batch_len > 0 {
            if let Err(e) = content_es
                .upload_contents(std::mem::take(current_batch))
                .await
            {
                error!(logger, "Error processing content: {:?}", e);
                return Err(e);
            } else {
                info!(
                    logger,
                    "Uploaded {} contents with size {} in {}ms",
                    current_batch_len,
                    ByteSize::b(current_batch_size).to_string(),
                    start.elapsed().as_millis(),
                );

                let elapsed = start.elapsed().as_millis() / current_batch_len as u128;
                STATS::content_upload_time_s.add_value(elapsed as i64, (reponame.clone(),));
                STATS::synced_contents.add_value(current_batch_len, (reponame.clone(),));
            }
        }

        while let Some(sender) = pending_messages.pop_front() {
            let res = sender.send(Ok(()));
            if let Err(e) = res {
                return Err(anyhow::anyhow!("Error sending content ready: {:?}", e));
            }
        }
        Ok(())
    }
}

impl Manager for ContentManager {
    fn start(
        mut self,
        ctx: CoreContext,
        reponame: String,
        content_es: Arc<dyn EdenapiSender + Send + Sync>,
        logger: Logger,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let content_recv = &mut self.content_recv;

            let mut pending_messages = VecDeque::new();
            let mut current_batch = Vec::new();
            let mut current_batch_size = 0;
            let mut flush_timer = interval(CONTENTS_FLUSH_INTERVAL);

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = content_recv.recv() => {
                        debug!(logger, "Content channel capacity: {} max capacity: {} in queue: {}", content_recv.capacity(), CONTENT_CHANNEL_SIZE,  content_recv.len());
                        STATS::contents_queue_capacity.set_value(ctx.fb, content_recv.capacity() as i64, (reponame.clone(),));
                        STATS::contents_queue_len.add_value(content_recv.len() as i64, (reponame.clone(),));
                        STATS::contents_queue_max_capacity.set_value(ctx.fb, content_recv.max_capacity() as i64, (reponame.clone(),));
                        match msg {
                            Some(ContentMessage::Content(ct_id, size)) => {
                                current_batch_size += size;
                                current_batch.push(ct_id);
                            }
                            Some(ContentMessage::ContentDone(files_sender, tree_sender)) => {
                                pending_messages.push_back(files_sender);
                                pending_messages.push_back(tree_sender);
                            }
                            None => break,
                        }

                        if current_batch_size >= MAX_BLOB_BYTES || current_batch.len() >= MAX_CONTENT_BATCH_SIZE {
                            if let Err(e) = ContentManager::flush_batch(&content_es, &mut current_batch, current_batch_size, &mut pending_messages, reponame.clone(), &logger).await {
                                error!(logger, "Error processing content: {:?}", e);
                                return;
                            }
                            current_batch_size = 0;
                        }
                    }
                    _ = flush_timer.tick() => {
                        if current_batch_size > 0 || !pending_messages.is_empty() {
                            if let Err(e) = ContentManager::flush_batch(&content_es, &mut current_batch, current_batch_size, &mut pending_messages, reponame.clone(), &logger).await {
                                error!(logger, "Error processing content: {:?}", e);
                                return;
                            }
                            current_batch_size = 0;
                        }
                    }
                }
            }
        });
    }
}
