/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bytesize::ByteSize;
use edenapi_types::AnyFileContentId;
use futures::channel::oneshot;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileContents;
use slog::error;
use slog::info;
use slog::Logger;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::time::interval;

use crate::sender::edenapi::EdenapiSender;

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

}

const CONTENT_CHANNEL_SIZE: usize = 8000;
const FILES_CHANNEL_SIZE: usize = 10000;
const TREES_CHANNEL_SIZE: usize = 10000;
const CHANGESET_CHANNEL_SIZE: usize = 5000;

const CHANGESETS_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const TREES_FLUSH_INTERVAL: Duration = Duration::from_secs(3);
const CONTENTS_FLUSH_INTERVAL: Duration = Duration::from_secs(3);

const MAX_CHANGESET_BATCH_SIZE: usize = 10;
const MAX_TREES_BATCH_SIZE: usize = 300;
const MAX_CONTENT_BATCH_SIZE: usize = 100;
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
    Content((AnyFileContentId, FileContents)),
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
    ChangesetDone(mpsc::Sender<Result<()>>),
    // Log changeset completion
    Log((String, Option<i64>)),
}

impl SendManager {
    pub fn new(external_sender: Arc<EdenapiSender>, logger: Logger, reponame: String) -> Self {
        // Create channel for receiving content
        let (content_sender, content_recv) = mpsc::channel(CONTENT_CHANNEL_SIZE);
        Self::spawn_content_sender(
            reponame.clone(),
            content_recv,
            external_sender.clone(),
            logger.clone(),
        );

        // Create channel for receiving files
        let (files_sender, files_recv) = mpsc::channel(FILES_CHANNEL_SIZE);
        Self::spawn_files_sender(
            reponame.clone(),
            files_recv,
            external_sender.clone(),
            logger.clone(),
        );

        // Create channel for receiving trees
        let (trees_sender, trees_recv) = mpsc::channel(TREES_CHANNEL_SIZE);
        Self::spawn_trees_sender(
            reponame.clone(),
            trees_recv,
            external_sender.clone(),
            logger.clone(),
        );

        // Create channel for receiving changesets
        let (changeset_sender, changeset_recv) = mpsc::channel(CHANGESET_CHANNEL_SIZE);
        Self::spawn_changeset_sender(
            reponame.clone(),
            changeset_recv,
            external_sender.clone(),
            logger.clone(),
        );

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
    ) {
        mononoke::spawn_task(async move {
            let mut pending_messages = VecDeque::new();
            let mut current_batch = Vec::new();
            let mut current_batch_size = 0;
            let mut flush_timer = interval(CONTENTS_FLUSH_INTERVAL);

            loop {
                tokio::select! {
                    msg = content_recv.recv() => {
                        match msg {
                            Some(ContentMessage::Content((ct_id, fcs))) => {
                                let size = fcs.size();
                                current_batch_size += size;
                                current_batch.push((ct_id, fcs));
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
                current_batch: &mut Vec<(AnyFileContentId, FileContents)>,
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
                        let elapsed = start.elapsed().as_secs() / current_batch_len as u64;

                        STATS::content_upload_time_s.add_value(elapsed as i64, (reponame.clone(),));
                        STATS::synced_contents.add_value(current_batch_len, (reponame.clone(),));
                        info!(
                            content_logger,
                            "Uploaded {} contents with size {} in {}s",
                            current_batch_len,
                            ByteSize::b(current_batch_size).to_string(),
                            elapsed
                        );
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

    fn spawn_files_sender(
        reponame: String,
        mut files_recv: mpsc::Receiver<FileMessage>,
        files_es: Arc<EdenapiSender>,
        files_logger: Logger,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;
            while let Some(msg) = files_recv.recv().await {
                match msg {
                    FileMessage::WaitForContents(receiver) => {
                        // Read outcome from content upload
                        let start = std::time::Instant::now();
                        match receiver.await {
                            Ok(Err(e)) => {
                                encountered_error.get_or_insert(e.context(
                                    "Contents error received. Winding down files sender.",
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
                    FileMessage::FileNode(f) if encountered_error.is_none() => {
                        // Upload the file nodes through sender
                        if let Err(e) = files_es.upload_filenodes(vec![(f)]).await {
                            encountered_error.get_or_insert(
                                e.context(format!("Failed to upload filenodes: {:?}", f)),
                            );
                        } else {
                            STATS::synced_filenodes.add_value(1, (reponame.clone(),));
                        }
                    }
                    FileMessage::FilesDone(sender) => {
                        if let Some(e) = encountered_error {
                            error!(files_logger, "Error processing files/trees: {:?}", e);
                            let _ = sender.send(Err(e));
                            return;
                        } else {
                            let res = sender.send(Ok(()));
                            if let Err(e) = res {
                                error!(files_logger, "Error sending content ready: {:?}", e);
                                return;
                            }
                        }
                    }
                    FileMessage::FileNode(_) => (),
                }
            }
        });
    }

    fn spawn_trees_sender(
        reponame: String,
        mut trees_recv: mpsc::Receiver<TreeMessage>,
        trees_es: Arc<EdenapiSender>,
        trees_logger: Logger,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;
            let mut batch_trees = Vec::new();
            let mut batch_done_senders = VecDeque::new();
            let mut timer = interval(TREES_FLUSH_INTERVAL);
            loop {
                tokio::select! {
                    msg = trees_recv.recv() => {
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
                    if let Some(e) = encountered_error {
                        let msg = format!("Error processing trees: {:?}", e);
                        while let Some(sender) = batch_done_senders.pop_front() {
                            let _ = sender.send(Err(anyhow::anyhow!(msg.clone())));
                        }
                        error!(trees_logger, "Error processing files/trees: {:?}", e);
                        return Err(anyhow::anyhow!(msg.clone()));
                    }

                    if let Err(e) = trees_es.upload_trees(std::mem::take(batch_trees)).await {
                        error!(trees_logger, "Failed to upload trees: {:?}", e);
                        return Err(e);
                    } else {
                        STATS::synced_trees
                            .add_value(batch_trees.len() as i64, (reponame.to_owned(),));
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
        reponame: String,
        mut changeset_recv: mpsc::Receiver<ChangesetMessage>,
        changeset_es: Arc<EdenapiSender>,
        changeset_logger: Logger,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;

            let mut pending_messages = VecDeque::new();
            let mut pending_log = VecDeque::new();

            let mut current_batch = Vec::new();
            let mut flush_timer = interval(CHANGESETS_FLUSH_INTERVAL);

            loop {
                tokio::select! {
                    msg = changeset_recv.recv() => {
                        match msg {
                            Some(ChangesetMessage::WaitForFilesAndTrees(files_receiver, trees_receiver)) => {
                                // Read outcome from files and trees upload
                                let start = std::time::Instant::now();
                                match tokio::try_join!(files_receiver, trees_receiver)  {
                                    Ok((res_files, res_trees))=> {
                                        if res_files.is_err() || res_trees.is_err() {
                                            error!(changeset_logger, "Error processing files/trees: {:?} {:?}", res_files, res_trees);
                                            encountered_error.get_or_insert(anyhow::anyhow!(
                                                "Files/trees error received. Winding down changesets sender.",
                                            ));
                                        }
                                        let elapsed = start.elapsed().as_secs();
                                        STATS::trees_files_wait_time_s.add_value(elapsed as i64, (reponame.clone(),));
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

                            Some(ChangesetMessage::ChangesetDone(sender))
                                if encountered_error.is_none() =>
                            {
                                pending_messages.push_back(sender);
                            }

                            Some(ChangesetMessage::Log((_, lag)))
                                if encountered_error.is_none() =>
                            {
                                pending_log.push_back(lag);
                            }

                            Some(ChangesetMessage::ChangesetDone(sender)) => {
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
                            | Some(ChangesetMessage::Changeset(_)) => {}

                            None => break,
                        }

                        if current_batch.len() >= MAX_CHANGESET_BATCH_SIZE {
                            if let Err(e) = flush_batch(
                                &changeset_es,
                                &mut current_batch,
                                &mut pending_messages,
                                &mut pending_log,
                                &changeset_logger,
                                reponame.clone(),
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
                    _ = flush_timer.tick() => {
                        if let Err(e) = flush_batch(
                            &changeset_es,
                            &mut current_batch,
                            &mut pending_messages,
                            &mut pending_log,
                            &changeset_logger,
                            reponame.clone(),
                        )
                        .await
                        {
                            return Err(anyhow::anyhow!("Error processing changesets: {:?}", e));
                        }
                    }
                }
            }

            async fn flush_batch(
                changeset_es: &Arc<EdenapiSender>,
                current_batch: &mut Vec<(HgBlobChangeset, BonsaiChangeset)>,
                pending_messages: &mut VecDeque<Sender<Result<(), anyhow::Error>>>,
                pending_log: &mut VecDeque<Option<i64>>,
                changeset_logger: &Logger,
                reponame: String,
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
