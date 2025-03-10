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
use tokio::sync::mpsc::Sender;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;
use crate::sync::MODERN_SYNC_BATCH_CHECKPOINT_NAME;

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

    contents_queue_len: dynamic_histogram("{}.contents.queue_len", (repo: String); 10, 0, crate::sender::manager::CONTENT_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    files_queue_len: dynamic_histogram("{}.files.queue_len", (repo: String); 10, 0, crate::sender::manager::FILES_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    trees_queue_len: dynamic_histogram("{}.trees.queue_len", (repo: String); 10, 0, crate::sender::manager::TREES_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
    changesets_queue_len: dynamic_histogram("{}.changesets.queue_len", (repo: String); 10, 0, crate::sender::manager::CHANGESET_CHANNEL_SIZE as u32, Average; P 50; P 75; P 95; P 99),
}

// Channel sizes
const CONTENT_CHANNEL_SIZE: usize = 30000;
const FILES_CHANNEL_SIZE: usize = 40000;
const TREES_CHANNEL_SIZE: usize = 40000;
const CHANGESET_CHANNEL_SIZE: usize = 15000;

// Flush intervals
const CHANGESETS_FLUSH_INTERVAL: Duration = Duration::from_secs(3);
const TREES_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const FILENODES_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const CONTENTS_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

// Batch sizes and limits
const MAX_CHANGESET_BATCH_SIZE: usize = 10;
const MAX_TREES_BATCH_SIZE: usize = 300;
const MAX_CONTENT_BATCH_SIZE: usize = 100;
const MAX_FILENODES_BATCH_SIZE: usize = 300;
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
    // Notify changeset sending is done
    ChangesetDone(
        Option<mpsc::Sender<Result<()>>>, // Channel to notify entry completion
        Option<u64>, // Changeset position within entry (assuming topological order)
    ),
    // Log changeset completion
    Log((String, Option<i64>)),
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
            reponame.clone(),
            content_recv,
            external_sender.clone(),
            logger.clone(),
            cancellation_requested.clone(),
        );

        // Create channel for receiving files
        let (files_sender, files_recv) = mpsc::channel(FILES_CHANNEL_SIZE);
        Self::spawn_filenodes_sender(
            reponame.clone(),
            files_recv,
            external_sender.clone(),
            logger.clone(),
            cancellation_requested.clone(),
        );

        // Create channel for receiving trees
        let (trees_sender, trees_recv) = mpsc::channel(TREES_CHANNEL_SIZE);
        Self::spawn_trees_sender(
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
                        STATS::contents_queue_len.add_value(content_recv.len() as i64, (reponame.clone(),));
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
                        STATS::trees_queue_len.add_value(trees_recv.len() as i64, (reponame.clone(),));
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

            let mut pending_messages = VecDeque::new();
            let mut pending_log = VecDeque::new();
            let mut pending_checkpoint = VecDeque::new();

            let mut current_batch = Vec::new();
            let mut flush_timer = interval(CHANGESETS_FLUSH_INTERVAL);

            while !cancellation_requested.load(Ordering::Relaxed) {
                tokio::select! {
                    msg = changeset_recv.recv() => {
                        debug!(changeset_logger, "Changeset channel capacity: {} max capacity: {} in queue: {}", changeset_recv.capacity(), CHANGESET_CHANNEL_SIZE,  changeset_recv.len());
                        STATS::changesets_queue_len.add_value(changeset_recv.len() as i64, (reponame.clone(),));
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

                            Some(ChangesetMessage::ChangesetDone(sender, position))
                                if encountered_error.is_none() =>
                            {
                                if let Some(sender) = sender {
                                    pending_messages.push_back(sender);
                                }
                                pending_checkpoint.push_back(position);
                            }

                            Some(ChangesetMessage::Log((_, lag)))
                                if encountered_error.is_none() =>
                            {
                                pending_log.push_back(lag);
                            }

                            Some(ChangesetMessage::ChangesetDone(Some(sender), _)) => {
                                let e = encountered_error.unwrap();
                                sender
                                    .send(Err(anyhow::anyhow!(
                                        "Error processing changesets: {:?}",
                                        e
                                    )))
                                    .await?;
                                return Err(e);
                            }

                            Some(ChangesetMessage::Log((_, _)))
                            | Some(ChangesetMessage::Changeset(_))
                            | Some(ChangesetMessage::ChangesetDone(_, _)) => {}

                            None => break,
                        }

                        if current_batch.len() >= MAX_CHANGESET_BATCH_SIZE {
                            if let Err(e) = flush_batch(
                                &ctx,
                                &changeset_es,
                                &mut current_batch,
                                &mut pending_messages,
                                &mut pending_log,
                                &changeset_logger,
                                reponame.clone(),
                                &mut pending_checkpoint,
                                mc.clone(),
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
                            &ctx,
                            &changeset_es,
                            &mut current_batch,
                            &mut pending_messages,
                            &mut pending_log,
                            &changeset_logger,
                            reponame.clone(),
                            &mut pending_checkpoint,
                            mc.clone(),
                        )
                        .await
                        {
                            return Err(anyhow::anyhow!("Error processing changesets: {:?}", e));
                        }
                    }
                }
            }

            async fn flush_batch(
                ctx: &CoreContext,
                changeset_es: &Arc<EdenapiSender>,
                current_batch: &mut Vec<(HgBlobChangeset, BonsaiChangeset)>,
                pending_messages: &mut VecDeque<Sender<Result<(), anyhow::Error>>>,
                pending_log: &mut VecDeque<Option<i64>>,
                changeset_logger: &Logger,
                reponame: String,
                pending_checkpoint: &mut VecDeque<Option<u64>>,
                mc: Arc<dyn MutableCounters + Send + Sync>,
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

                let position = pending_checkpoint.pop_back();
                if let Some(Some(position)) = position {
                    info!(changeset_logger, "Setting checkpoint to {}", position);
                    let res = mc
                        .set_counter(
                            ctx,
                            MODERN_SYNC_BATCH_CHECKPOINT_NAME,
                            position.try_into().unwrap(),
                            None,
                        )
                        .await?;

                    if !res {
                        warn!(changeset_logger, "Failed to set checkpoint: {:?}", res);
                    }
                }
                pending_checkpoint.drain(..);

                while let Some(sender) = pending_messages.pop_front() {
                    let res = sender.send(Ok(()));
                    if let Err(e) = res.await {
                        return Err(anyhow::anyhow!("Error sending content ready: {:?}", e));
                    }
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
}
