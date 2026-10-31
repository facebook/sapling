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

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use bytes::BytesMut;
use bytesize::ByteSize;
use cloned::cloned;
use context::CoreContext;
use edenapi_types::AnyFileContentId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::channel::oneshot;
use futures::stream;
use metaconfig_types::ModernSyncChannelConfig;
use mononoke_macros::mononoke;
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::ContentMessage;
use crate::sender::manager::LoopAction;
use crate::sender::manager::Manager;

define_stats! {
    prefix = "mononoke.modern_sync.manager.content";

    synced_contents:  dynamic_timeseries("{}.synced_contents", (repo: String); Sum),
    content_upload_time_s:  dynamic_timeseries("{}.content_upload_time_ms", (repo: String); Average),

    contents_queue_capacity: dynamic_singleton_counter("{}.contents.queue_capacity", (repo: String)),
    contents_queue_len: dynamic_quantile_stat("{}.contents.queue_len", (repo: String); Average; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
    contents_queue_max_capacity: dynamic_singleton_counter("{}.contents.queue_max_capacity", (repo: String)),
}

pub(crate) struct ContentManager {
    max_blob_bytes: u64,
    config: ModernSyncChannelConfig,
    content_recv: mpsc::Receiver<ContentMessage>,
    repo_blobstore: RepoBlobstore,
}

impl ContentManager {
    pub(crate) fn new(
        max_blob_bytes: u64,
        config: ModernSyncChannelConfig,
        content_recv: mpsc::Receiver<ContentMessage>,
        repo_blobstore: RepoBlobstore,
    ) -> Self {
        Self {
            max_blob_bytes,
            config,
            content_recv,
            repo_blobstore,
        }
    }
}

struct ContentRunner {
    ctx: CoreContext,
    reponame: String,
    content_es: Arc<dyn EdenapiSender + Send + Sync>,
    cancellation_requested: Arc<AtomicBool>,
    max_blob_bytes: u64,
    config: ModernSyncChannelConfig,
    repo_blobstore: RepoBlobstore,
    pending_messages: VecDeque<oneshot::Sender<Result<()>>>,
    current_batch: Vec<ContentId>,
    current_batch_size: u64,
}

impl ContentRunner {
    fn new(
        ctx: CoreContext,
        reponame: String,
        content_es: Arc<dyn EdenapiSender + Send + Sync>,
        cancellation_requested: Arc<AtomicBool>,
        max_blob_bytes: u64,
        config: ModernSyncChannelConfig,
        repo_blobstore: RepoBlobstore,
    ) -> Self {
        Self {
            ctx,
            reponame,
            content_es,
            cancellation_requested,
            max_blob_bytes,
            config,
            repo_blobstore,
            pending_messages: VecDeque::new(),
            current_batch: Vec::new(),
            current_batch_size: 0,
        }
    }

    async fn run(&mut self, content_recv: &mut mpsc::Receiver<ContentMessage>) {
        let mut flush_timer = interval(Duration::from_millis(self.config.flush_interval_ms as u64));
        while !self.cancelled() {
            let action = tokio::select! {
                msg = content_recv.recv() => {
                    self.record_queue_stats(content_recv);
                    self.handle_message(msg).await
                }
                _ = flush_timer.tick() => self.handle_tick().await,
            };
            match action {
                LoopAction::Continue => {}
                LoopAction::Exit => return,
            }
        }
    }

    async fn handle_message(&mut self, msg: Option<ContentMessage>) -> LoopAction {
        match msg {
            Some(ContentMessage::Content(ct_id, size)) => {
                self.current_batch_size += size;
                self.current_batch.push(ct_id);
            }
            Some(ContentMessage::ContentDone(files_sender, tree_sender)) => {
                self.pending_messages.push_back(files_sender);
                self.pending_messages.push_back(tree_sender);
            }
            None => return LoopAction::Exit,
        }

        if self.current_batch_size >= self.max_blob_bytes
            || self.current_batch.len() >= self.config.batch_size as usize
        {
            if let Err(e) = self.flush_batch().await {
                tracing::error!("Error processing content: {:?}", e);
                return LoopAction::Exit;
            }
        }
        LoopAction::Continue
    }

    async fn handle_tick(&mut self) -> LoopAction {
        if self.current_batch_size > 0 || !self.pending_messages.is_empty() {
            if let Err(e) = self.flush_batch().await {
                tracing::error!("Error processing content: {:?}", e);
                return LoopAction::Exit;
            }
        }
        LoopAction::Continue
    }

    async fn flush_batch(&mut self) -> Result<(), anyhow::Error> {
        let current_batch_len = self.current_batch.len();
        let current_batch_size = self.current_batch_size;
        let start = std::time::Instant::now();

        if current_batch_len > 0 {
            let contents = std::mem::take(&mut self.current_batch);
            let repo_blobstore = self.repo_blobstore.clone();
            let ctx = &self.ctx;

            let full_items = stream::iter(contents)
                .map(|id| {
                    cloned!(repo_blobstore);
                    async move {
                        let bytes = filestore::fetch(repo_blobstore, ctx, &id.into())
                            .await?
                            .ok_or(anyhow!("Content is not found (which should never happen"))?
                            .try_collect::<BytesMut>()
                            .await?;
                        Ok::<_, Error>((
                            AnyFileContentId::ContentId(id.into()),
                            bytes.freeze().into(),
                        ))
                    }
                })
                .buffer_unordered(current_batch_len)
                .try_collect::<Vec<(AnyFileContentId, minibytes::Bytes)>>()
                .await?;

            if let Err(e) = self.content_es.upload_contents(full_items).await {
                tracing::error!("Error processing content: {:?}", e);
                return Err(e);
            } else {
                tracing::info!(
                    "Uploaded {} contents with size {} in {}ms",
                    current_batch_len,
                    ByteSize::b(current_batch_size).to_string(),
                    start.elapsed().as_millis(),
                );

                let elapsed = start.elapsed().as_millis() / current_batch_len as u128;
                STATS::content_upload_time_s.add_value(elapsed as i64, (self.reponame.clone(),));
                STATS::synced_contents
                    .add_value(current_batch_len as i64, (self.reponame.clone(),));
            }
        }

        while let Some(sender) = self.pending_messages.pop_front() {
            let res = sender.send(Ok(()));
            if let Err(e) = res {
                return Err(anyhow::anyhow!("Error sending content ready: {e:?}"));
            }
        }
        self.current_batch_size = 0;
        Ok(())
    }

    fn record_queue_stats(&self, content_recv: &mpsc::Receiver<ContentMessage>) {
        tracing::debug!(
            "Content channel capacity: {} max capacity: {} in queue: {}",
            content_recv.capacity(),
            self.config.channel_size,
            content_recv.len()
        );
        STATS::contents_queue_capacity.set_value(
            self.ctx.fb,
            content_recv.capacity() as i64,
            (self.reponame.clone(),),
        );
        STATS::contents_queue_len.add_value(content_recv.len() as i64, (self.reponame.clone(),));
        STATS::contents_queue_max_capacity.set_value(
            self.ctx.fb,
            content_recv.max_capacity() as i64,
            (self.reponame.clone(),),
        );
    }

    fn cancelled(&self) -> bool {
        self.cancellation_requested.load(Ordering::Relaxed)
    }
}

impl Manager for ContentManager {
    fn start(
        self,
        ctx: CoreContext,
        reponame: String,
        content_es: Arc<dyn EdenapiSender + Send + Sync>,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let Self {
                max_blob_bytes,
                config,
                mut content_recv,
                repo_blobstore,
            } = self;
            let mut runner = ContentRunner::new(
                ctx,
                reponame,
                content_es,
                cancellation_requested,
                max_blob_bytes,
                config,
                repo_blobstore,
            );
            runner.run(&mut content_recv).await;
        });
    }
}
