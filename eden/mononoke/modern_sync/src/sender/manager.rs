/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bytesize::ByteSize;
use context::CoreContext;
use futures::channel::oneshot;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentId;
use mutable_counters::MutableCounters;
use slog::debug;
use slog::error;
use slog::info;
use slog::warn;
use slog::Logger;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;

pub(crate) const MODERN_SYNC_COUNTER_NAME: &str = "modern_sync";
pub(crate) const MODERN_SYNC_BATCH_CHECKPOINT_NAME: &str = "modern_sync_batch_checkpoint";
pub(crate) const MODERN_SYNC_CURRENT_ENTRY_ID: &str = "modern_sync_batch_id";

define_stats! {
    prefix = "mononoke.modern_sync";
    synced_commits:  dynamic_timeseries("{}.commits_synced", (repo: String); Sum),
    synced_contents:  dynamic_timeseries("{}.synced_contents", (repo: String); Sum),
    synced_trees:  dynamic_timeseries("{}.synced_trees", (repo: String); Sum),
    synced_filenodes:  dynamic_timeseries("{}.synced_filenodes", (repo: String); Sum),
    sync_lag_seconds:  dynamic_timeseries("{}.sync_lag_seconds", (repo: String); Average),
    content_wait_time_s:  dynamic_timeseries("{}.content_wait_time_s", (repo: String); Average),
    trees_files_wait_time_s:  dynamic_timeseries("{}.trees_files_wait_time_s", (repo: String); Average),
    changeset_upload_time_s:  dynamic_timeseries("{}.changeset_upload_time_s", (repo: String); Average),
    content_upload_time_s:  dynamic_timeseries("{}.content_upload_time_ms", (repo: String); Average),

    contents_queue_capacity: dynamic_singleton_counter("{}.contents.queue_capacity", (repo: String)),
    contents_queue_len: dynamic_histogram("{}.contents.queue_len", (repo: String); 10, 0, crate::sender::manager::CONTENT_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    contents_queue_max_capacity: dynamic_singleton_counter("{}.contents.queue_max_capacity", (repo: String)),
    files_queue_capacity: dynamic_singleton_counter("{}.files.queue_capacity", (repo: String)),
    files_queue_len: dynamic_histogram("{}.files.queue_len", (repo: String); 10, 0, crate::sender::manager::FILES_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    files_queue_max_capacity: dynamic_singleton_counter("{}.files.queue_max_capacity", (repo: String)),
    trees_queue_capacity: dynamic_singleton_counter("{}.trees.queue_capacity", (repo: String)),
    trees_queue_len: dynamic_histogram("{}.trees.queue_len", (repo: String); 10, 0, crate::sender::manager::TREES_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    trees_queue_max_capacity: dynamic_singleton_counter("{}.trees.queue_max_capacity", (repo: String)),
    changesets_queue_capacity: dynamic_singleton_counter("{}.changesets.queue_capacity", (repo: String)),
    changesets_queue_len: dynamic_histogram("{}.changesets.queue_len", (repo: String); 10, 0, crate::sender::manager::CHANGESET_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    changesets_queue_max_capacity: dynamic_singleton_counter("{}.changesets.queue_max_capacity", (repo: String)),
}

// Channel sizes
const CONTENT_CHANNEL_SIZE: usize = 40_000;
const FILES_CHANNEL_SIZE: usize = 50_000;
const TREES_CHANNEL_SIZE: usize = 50_000;
const CHANGESET_CHANNEL_SIZE: usize = 15_000;

// Flush intervals
// This indicates how often we flush the content, trees, files and changesets
// despite the channel not being full. This is to ensure that we don't get stuck
// waiting for the channel to be full with unflushed data.
const CHANGESETS_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const TREES_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const FILENODES_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const CONTENTS_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

// Batch sizes and limits
const MAX_CHANGESET_BATCH_SIZE: usize = 20;
const MAX_TREES_BATCH_SIZE: usize = 500;
const MAX_CONTENT_BATCH_SIZE: usize = 300;
const MAX_FILENODES_BATCH_SIZE: usize = 500;
const MAX_BLOB_BYTES: u64 = 10 * 10 * 1024 * 1024; // 100 MB

#[derive(Clone)]
pub struct SendManager {
    content_sender: mpsc::Sender<ContentMessage>,
    files_sender: mpsc::Sender<FileMessage>,
    trees_sender: mpsc::Sender<TreeMessage>,
    changeset_sender: mpsc::Sender<ChangesetMessage>,
}

pub enum ContentMessage {
    // Send the content to remote end
    Content(ContentId, u64),
    // Finished sending content of a changeset. Go ahead with files and trees
    ContentDone(oneshot::Sender<Result<()>>, oneshot::Sender<Result<()>>),
}

#[derive(Default)]
pub struct Messages {
    pub content_messages: Vec<ContentMessage>,
    pub trees_messages: Vec<TreeMessage>,
    pub files_messages: Vec<FileMessage>,
    pub changeset_messages: Vec<ChangesetMessage>,
}

pub enum TreeMessage {
    // Wait for contents to be sent before sending trees
    WaitForContents(oneshot::Receiver<Result<()>>),
    // Send the tree to remote end
    Tree(HgManifestId),
    // Finished sending trees. Go ahead with changesets
    TreesDone(oneshot::Sender<Result<()>>),
}

pub enum FileMessage {
    // Wait for contents to be sent before sending files
    WaitForContents(oneshot::Receiver<Result<()>>),
    // Send the file node to remote end
    FileNode(HgFileNodeId),
    // Finished sending files. Go ahead with changesets
    FilesDone(oneshot::Sender<Result<()>>),
}

pub enum ChangesetMessage {
    // Wait for files and trees to be sent before sending changesets
    WaitForFilesAndTrees(oneshot::Receiver<Result<()>>, oneshot::Receiver<Result<()>>),
    // Send the changeset to remote end
    Changeset((HgBlobChangeset, BonsaiChangeset)),
    // Checkpoint position (first argument) within the BUL entry (second argument)
    CheckpointInEntry(u64, i64),
    // Perfrom bookmark movement and mark BUL entry as completed once the changeset is synced
    FinishEntry(BookmarkInfo, i64),
    // Notify changeset sending is done
    NotifyCompletion(oneshot::Sender<Result<()>>),
    // Log changeset completion
    Log((String, Option<i64>)),
}

pub struct BookmarkInfo {
    pub name: String,
    pub from_cs_id: Option<HgChangesetId>,
    pub to_cs_id: Option<HgChangesetId>,
}

impl SendManager {
    pub fn new(
        ctx: CoreContext,
        external_sender: Arc<EdenapiSender>,
        logger: Logger,
        reponame: String,
        exit_file: PathBuf,
        mc: Arc<dyn MutableCounters + Send + Sync>,
    ) -> Self {
        let cancellation_requested = Arc::new(AtomicBool::new(false));

        // Create channel for receiving content
        let (content_sender, content_recv) = mpsc::channel(CONTENT_CHANNEL_SIZE);
        Self::spawn_content_sender(
            ctx.clone(),
            reponame.clone(),
            content_recv,
            external_sender.clone(),
            logger.clone(),
            cancellation_requested.clone(),
        );

        // Create channel for receiving files
        let (files_sender, files_recv) = mpsc::channel(FILES_CHANNEL_SIZE);
        Self::spawn_filenodes_sender(
            ctx.clone(),
            reponame.clone(),
            files_recv,
            external_sender.clone(),
            logger.clone(),
            cancellation_requested.clone(),
        );

        // Create channel for receiving trees
        let (trees_sender, trees_recv) = mpsc::channel(TREES_CHANNEL_SIZE);
        Self::spawn_trees_sender(
            ctx.clone(),
            reponame.clone(),
            trees_recv,
            external_sender.clone(),
            logger.clone(),
            cancellation_requested.clone(),
        );

        // Create channel for receiving changesets
        let (changeset_sender, changeset_recv) = mpsc::channel(CHANGESET_CHANNEL_SIZE);
        Self::spawn_changeset_sender(
            ctx,
            reponame,
            changeset_recv,
            external_sender,
            logger.clone(),
            cancellation_requested.clone(),
            mc,
        );

        mononoke::spawn_task(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                if fs::metadata(exit_file.clone()).is_ok() {
                    warn!(logger, "Exit file detected, stopping sync");
                    cancellation_requested.store(true, Ordering::Relaxed);
                    break;
                }
            }
        });

        Self {
            content_sender,
            files_sender,
            trees_sender,
            changeset_sender,
        }
    }

    fn spawn_content_sender(
        ctx: CoreContext,
        reponame: String,
        mut content_recv: mpsc::Receiver<ContentMessage>,
        content_es: Arc<EdenapiSender>,
        content_logger: Logger,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let mut pending_messages = VecDeque::new();
            let mut current_batch = Vec::new();
            let mut current_batch_size = 0;
            let mut flush_timer = interval(CONTENTS_FLUSH_INTERVAL);

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = content_recv.recv() => {
                        debug!(content_logger, "Content channel capacity: {} max capacity: {} in queue: {}", content_recv.capacity(), CONTENT_CHANNEL_SIZE,  content_recv.len());
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
                            if let Err(e) = flush_batch(&content_es, &mut current_batch, current_batch_size, &mut pending_messages, &content_logger, reponame.clone()).await {
                                error!(content_logger, "Error processing content: {:?}", e);
                                return;
                            }
                            current_batch_size = 0;
                        }
                    }
                    _ = flush_timer.tick() => {
                        if current_batch_size > 0 || !pending_messages.is_empty() {
                            if let Err(e) = flush_batch(&content_es, &mut current_batch,current_batch_size,  &mut pending_messages, &content_logger, reponame.clone()).await {
                                error!(content_logger, "Error processing content: {:?}", e);
                                return;
                            }
                            current_batch_size = 0;
                        }
                    }
                }
            }

            async fn flush_batch(
                content_es: &Arc<EdenapiSender>,
                current_batch: &mut Vec<ContentId>,
                current_batch_size: u64,
                pending_messages: &mut VecDeque<oneshot::Sender<Result<(), anyhow::Error>>>,
                content_logger: &Logger,
                reponame: String,
            ) -> Result<(), anyhow::Error> {
                let current_batch_len = current_batch.len() as i64;
                let start = std::time::Instant::now();
                if current_batch_len > 0 {
                    if let Err(e) = content_es
                        .upload_contents(std::mem::take(current_batch))
                        .await
                    {
                        error!(content_logger, "Error processing content: {:?}", e);
                        return Err(e);
                    } else {
                        info!(
                            content_logger,
                            "Uploaded {} contents with size {} in {}ms",
                            current_batch_len,
                            ByteSize::b(current_batch_size).to_string(),
                            start.elapsed().as_millis(),
                        );

                        let elapsed = start.elapsed().as_secs() / current_batch_len as u64;
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
        });
    }

    fn spawn_filenodes_sender(
        ctx: CoreContext,
        reponame: String,
        mut filenodes_recv: mpsc::Receiver<FileMessage>,
        filenodes_es: Arc<EdenapiSender>,
        filenodes_logger: Logger,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;
            let mut batch_filenodes = Vec::new();
            let mut batch_done_senders = VecDeque::new();
            let mut timer = interval(FILENODES_FLUSH_INTERVAL);

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = filenodes_recv.recv() => {
                        debug!(filenodes_logger, "Filenodes channel capacity: {} max capacity: {} in queue: {}", filenodes_recv.capacity(), FILES_CHANNEL_SIZE,  filenodes_recv.len());
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
                        if batch_filenodes.len() >= MAX_FILENODES_BATCH_SIZE {
                            if let Err(e) = flush_filenodes(&filenodes_es, &mut batch_filenodes, &mut batch_done_senders, &mut encountered_error, &reponame, &filenodes_logger).await {
                                error!(filenodes_logger, "Filenodes flush failed: {:?}", e);
                                return;
                            }
                        }
                    }
                    _ = timer.tick() => {
                        if let Err(e) = flush_filenodes(&filenodes_es, &mut batch_filenodes, &mut batch_done_senders, &mut encountered_error, &reponame, &filenodes_logger).await {
                            error!(filenodes_logger, "Filenodes flush failed: {:?}", e);
                            return;
                        }
                    }
                }
            }

            async fn flush_filenodes(
                filenodes_es: &Arc<EdenapiSender>,
                batch_filenodes: &mut Vec<HgFileNodeId>,
                batch_done_senders: &mut VecDeque<oneshot::Sender<Result<()>>>,
                encountered_error: &mut Option<anyhow::Error>,
                reponame: &str,
                filenodes_logger: &Logger,
            ) -> Result<(), anyhow::Error> {
                if !batch_filenodes.is_empty() || !batch_done_senders.is_empty() {
                    let batch_size = batch_filenodes.len() as i64;
                    if let Some(e) = encountered_error {
                        let msg = format!("Error processing filenodes: {:?}", e);
                        while let Some(sender) = batch_done_senders.pop_front() {
                            let _ = sender.send(Err(anyhow::anyhow!(msg.clone())));
                        }
                        error!(filenodes_logger, "Error processing filenodes: {:?}", e);
                        return Err(anyhow::anyhow!(msg.clone()));
                    }

                    if !batch_filenodes.is_empty() {
                        let start = std::time::Instant::now();
                        if let Err(e) = filenodes_es
                            .upload_filenodes(std::mem::take(batch_filenodes))
                            .await
                        {
                            error!(filenodes_logger, "Failed to upload filenodes: {:?}", e);
                            return Err(e);
                        } else {
                            info!(
                                filenodes_logger,
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
                            error!(filenodes_logger, "{}", msg);
                            return Err(anyhow::anyhow!(msg));
                        }
                    }
                }
                Ok(())
            }
        });
    }

    fn spawn_trees_sender(
        ctx: CoreContext,
        reponame: String,
        mut trees_recv: mpsc::Receiver<TreeMessage>,
        trees_es: Arc<EdenapiSender>,
        trees_logger: Logger,
        cancellation_requested: Arc<AtomicBool>,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;
            let mut batch_trees = Vec::new();
            let mut batch_done_senders = VecDeque::new();
            let mut timer = interval(TREES_FLUSH_INTERVAL);
            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = trees_recv.recv() => {
                        debug!(trees_logger, "Trees channel capacity: {} max capacity: {} in queue: {}", trees_recv.capacity(), TREES_CHANNEL_SIZE,  trees_recv.len());
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
                        if batch_trees.len() >= MAX_TREES_BATCH_SIZE {
                            if let Err(e) = flush_trees(&trees_es, &mut batch_trees, &mut batch_done_senders, &mut encountered_error, &reponame,  &trees_logger).await {
                                error!(trees_logger, "Trees flush failed: {:?}", e);
                                return;
                            }
                        }
                    }
                    _ = timer.tick() => {
                        if let Err(e) = flush_trees(&trees_es, &mut batch_trees, &mut batch_done_senders, &mut encountered_error, &reponame, &trees_logger).await {
                            error!(trees_logger, "Trees flush failed: {:?}", e);
                            return;
                        }
                    }
                }
            }
            async fn flush_trees(
                trees_es: &Arc<EdenapiSender>,
                batch_trees: &mut Vec<HgManifestId>,
                batch_done_senders: &mut VecDeque<oneshot::Sender<Result<()>>>,
                encountered_error: &mut Option<anyhow::Error>,
                reponame: &str,
                trees_logger: &Logger,
            ) -> Result<(), anyhow::Error> {
                if !batch_trees.is_empty() || !batch_done_senders.is_empty() {
                    let batch_size = batch_trees.len() as i64;
                    if let Some(e) = encountered_error {
                        let msg = format!("Error processing trees: {:?}", e);
                        while let Some(sender) = batch_done_senders.pop_front() {
                            let _ = sender.send(Err(anyhow::anyhow!(msg.clone())));
                        }
                        error!(trees_logger, "Error processing files/trees: {:?}", e);
                        return Err(anyhow::anyhow!(msg.clone()));
                    }

                    if !batch_trees.is_empty() {
                        let start = std::time::Instant::now();
                        if let Err(e) = trees_es.upload_trees(std::mem::take(batch_trees)).await {
                            error!(trees_logger, "Failed to upload trees: {:?}", e);
                            return Err(e);
                        } else {
                            info!(
                                trees_logger,
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
                            error!(trees_logger, "{}", msg);
                            return Err(anyhow::anyhow!(msg));
                        }
                    }
                }
                Ok(())
            }
        });
    }

    fn spawn_changeset_sender(
        ctx: CoreContext,
        reponame: String,
        mut changeset_recv: mpsc::Receiver<ChangesetMessage>,
        changeset_es: Arc<EdenapiSender>,
        changeset_logger: Logger,
        cancellation_requested: Arc<AtomicBool>,
        mc: Arc<dyn MutableCounters + Send + Sync>,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;

            let mut pending_log = VecDeque::new();

            let mut latest_in_entry_checkpoint = None;
            let mut latest_entry_id = None;
            let mut latest_bookmark: Option<BookmarkInfo> = None;
            let mut pending_notification = None;

            let mut current_batch = Vec::new();
            let mut flush_timer = interval(CHANGESETS_FLUSH_INTERVAL);

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {

                    msg = changeset_recv.recv() => {

                        debug!(
                            changeset_logger,
                            "Changeset channel capacity: {} max capacity: {} in queue: {}",
                            changeset_recv.capacity(),
                            CHANGESET_CHANNEL_SIZE,
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
                                            error!(
                                                changeset_logger,
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

                        if current_batch.len() >= MAX_CHANGESET_BATCH_SIZE {
                            if let Err(e) = flush_batch(
                                reponame.clone(),
                                &ctx,
                                &changeset_logger,
                                &changeset_es,
                                mc.clone(),
                                &mut current_batch,
                                &mut pending_log,
                                &mut latest_in_entry_checkpoint,
                                &mut latest_entry_id,
                                &mut latest_bookmark,
                                &mut pending_notification,
                            )
                            .await
                            {
                                return Err(anyhow::anyhow!(
                                    "Error processing changesets: {:?}",
                                    e
                                ));
                            }
                        }
                    }
                    _ = flush_timer.tick() =>
                    {
                        if let Err(e) = flush_batch(
                            reponame.clone(),
                            &ctx,
                            &changeset_logger,
                            &changeset_es,
                            mc.clone(),
                            &mut current_batch,
                            &mut pending_log,
                            &mut latest_in_entry_checkpoint,
                            &mut latest_entry_id,
                            &mut latest_bookmark,
                            &mut pending_notification,
                        )
                        .await
                        {
                            return Err(anyhow::anyhow!("Error processing changesets: {:?}", e));
                        }
                    }
                }
            }

            async fn flush_batch(
                reponame: String,
                ctx: &CoreContext,
                changeset_logger: &Logger,
                changeset_es: &Arc<EdenapiSender>,
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
                        error!(changeset_logger, "Failed to upload changesets: {:?}", e);
                        return Err(e);
                    } else {
                        let elapsed = start.elapsed().as_secs() / batch_size as u64;
                        STATS::changeset_upload_time_s
                            .add_value(elapsed as i64, (reponame.clone(),));
                        STATS::synced_commits.add_value(batch_size as i64, (reponame.clone(),));
                    }
                }

                while let Some(Some(lag)) = pending_log.pop_front() {
                    STATS::sync_lag_seconds.add_value(lag, (reponame.clone(),));
                }

                if let Some((position, id)) = latest_checkpoint.take() {
                    info!(
                        changeset_logger,
                        "Setting checkpoint from entry {} to {}", id, position
                    );

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
                        warn!(
                            changeset_logger,
                            "Failed to checkpoint entry {} at position {:?}", id, position
                        );
                    }
                }

                if let Some(info) = latest_bookmark.take() {
                    info!(
                        changeset_logger,
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
                    info!(changeset_logger, "Marking entry {} as done", id);
                    let res = mc
                        .set_counter(ctx, MODERN_SYNC_COUNTER_NAME, id, None)
                        .await?;

                    if !res {
                        warn!(changeset_logger, "Failed to mark entry {} as synced", id);
                    }
                }

                if let Some(sender) = pending_notification.take() {
                    let _ = sender.send(Ok(()));
                }

                Ok(())
            }

            Ok(())
        });
    }

    pub async fn send_content(&self, content_msg: ContentMessage) -> Result<()> {
        self.content_sender
            .send(content_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_file(&self, ft_msg: FileMessage) -> Result<()> {
        self.files_sender
            .send(ft_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_tree(&self, ft_msg: TreeMessage) -> Result<()> {
        self.trees_sender
            .send(ft_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_changeset(&self, cs_msg: ChangesetMessage) -> Result<()> {
        self.changeset_sender
            .send(cs_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_contents(&self, content_msgs: Vec<ContentMessage>) -> Result<()> {
        for content_msg in content_msgs {
            self.send_content(content_msg).await?;
        }
        Ok(())
    }

    pub async fn send_files(&self, ft_msgs: Vec<FileMessage>) -> Result<()> {
        for ft_msg in ft_msgs {
            self.send_file(ft_msg).await?;
        }
        Ok(())
    }

    pub async fn send_trees(&self, ft_msgs: Vec<TreeMessage>) -> Result<()> {
        for ft_msg in ft_msgs {
            self.send_tree(ft_msg).await?;
        }
        Ok(())
    }

    pub async fn send_changesets(&self, cs_msgs: Vec<ChangesetMessage>) -> Result<()> {
        for cs_msg in cs_msgs {
            self.send_changeset(cs_msg).await?;
        }
        Ok(())
    }
}
