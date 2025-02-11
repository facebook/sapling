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
use edenapi_types::AnyFileContentId;
use futures::channel::oneshot;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileContents;
use slog::error;
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

}

const CONTENT_CHANNEL_SIZE: usize = 1000;
const FILES_AND_TREES_CHANNEL_SIZE: usize = 1000;
const CHANGESET_CHANNEL_SIZE: usize = 1000;

const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_CHANGESET_BATCH_SIZE: usize = 5;

#[derive(Clone)]
pub struct SendManager {
    content_sender: mpsc::Sender<ContentMessage>,
    files_and_trees_sender: mpsc::Sender<FileOrTreeMessage>,
    changeset_sender: mpsc::Sender<ChangesetMessage>,
}

pub enum ContentMessage {
    // Send the content to remote end
    Content((AnyFileContentId, FileContents)),
    // Finished sending content of a changeset. Go ahead with files and trees
    ContentDone(oneshot::Sender<Result<()>>),
}

pub enum FileOrTreeMessage {
    // Wait for contents to be sent before sending files and trees
    WaitForContents(oneshot::Receiver<Result<()>>),
    // Send the file node to remote end
    FileNode(HgFileNodeId),
    // Send the tree to remote end
    Tree(HgManifestId),
    // Finished sending files and trees. Go ahead with changesets
    FilesAndTreesDone(oneshot::Sender<Result<()>>),
}

pub enum ChangesetMessage {
    // Wait for files and trees to be sent before sending changesets
    WaitForFilesAndTrees(oneshot::Receiver<Result<()>>),
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

        // Create channel for receiving files and trees
        let (files_and_trees_sender, files_and_trees_recv) =
            mpsc::channel(FILES_AND_TREES_CHANNEL_SIZE);
        Self::spawn_files_and_trees_sender(
            reponame.clone(),
            files_and_trees_recv,
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
            files_and_trees_sender,
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
            let mut encountered_error: Option<anyhow::Error> = None;
            while let Some(msg) = content_recv.recv().await {
                match msg {
                    ContentMessage::Content((ct_id, fcs)) => {
                        // Upload the content through sender
                        if let Err(e) = content_es.upload_contents(vec![(ct_id, fcs)]).await {
                            encountered_error.get_or_insert(
                                e.context(format!("Failed to upload content: {:?}", ct_id)),
                            );
                        }
                        STATS::synced_contents.add_value(1, (reponame.clone(),));
                    }
                    ContentMessage::ContentDone(sender) => {
                        if let Some(e) = encountered_error {
                            error!(content_logger, "Error processing content: {:?}", e);
                            let _ = sender.send(Err(e));
                            return;
                        } else {
                            let res = sender.send(Ok(()));
                            if let Err(e) = res {
                                error!(content_logger, "Error sending content ready: {:?}", e);
                                return;
                            }
                        }
                    }
                }
            }
        });
    }

    fn spawn_files_and_trees_sender(
        reponame: String,
        mut files_and_trees_recv: mpsc::Receiver<FileOrTreeMessage>,
        files_trees_es: Arc<EdenapiSender>,
        files_trees_logger: Logger,
    ) {
        mononoke::spawn_task(async move {
            let mut encountered_error: Option<anyhow::Error> = None;
            while let Some(msg) = files_and_trees_recv.recv().await {
                match msg {
                    FileOrTreeMessage::WaitForContents(receiver) => {
                        // Read outcome from content upload
                        let start = std::time::Instant::now();
                        match receiver.await {
                            Ok(Err(e)) => {
                                encountered_error.get_or_insert(e.context(
                                    "Contents error received. Winding down files/trees sender.",
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
                    FileOrTreeMessage::FileNode(f) if encountered_error.is_none() => {
                        // Upload the file nodes through sender
                        if let Err(e) = files_trees_es.upload_filenodes(vec![(f)]).await {
                            encountered_error.get_or_insert(
                                e.context(format!("Failed to upload filenodes: {:?}", f)),
                            );
                        }
                        STATS::synced_filenodes.add_value(1, (reponame.clone(),));
                    }
                    FileOrTreeMessage::Tree(t) if encountered_error.is_none() => {
                        // Upload the trees through sender
                        if let Err(e) = files_trees_es.upload_trees(vec![t]).await {
                            encountered_error.get_or_insert(
                                e.context(format!("Failed to upload trees: {:?}", t)),
                            );
                        }
                        STATS::synced_trees.add_value(1, (reponame.clone(),));
                    }
                    FileOrTreeMessage::FilesAndTreesDone(sender) => {
                        if let Some(e) = encountered_error {
                            error!(files_trees_logger, "Error processing files/trees: {:?}", e);
                            let _ = sender.send(Err(e));
                            return;
                        } else {
                            let res = sender.send(Ok(()));
                            if let Err(e) = res {
                                error!(files_trees_logger, "Error sending content ready: {:?}", e);
                                return;
                            }
                        }
                    }
                    FileOrTreeMessage::FileNode(_) | FileOrTreeMessage::Tree(_) => (),
                }
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
            let mut flush_timer = interval(FLUSH_INTERVAL);

            loop {
                tokio::select! {
                    msg = changeset_recv.recv()=>{
                        match msg {
                            Some(ChangesetMessage::WaitForFilesAndTrees(receiver)) => {
                                // Read outcome from files and trees upload
                                let start = std::time::Instant::now();
                                match receiver.await {
                                    Ok(Err(e)) => {
                                        encountered_error.get_or_insert(e.context(
                                            "Files/trees error received. Winding down changesets sender.",
                                        ));
                                    }
                                    Err(e) => {
                                        encountered_error.get_or_insert(anyhow::anyhow!(
                                            "Error waiting for files/trees error received {:#}", e)
                                        );
                                    }
                                    _ => (),
                                }
                                let elapsed = start.elapsed().as_secs();
                                STATS::trees_files_wait_time_s
                                    .add_value(elapsed as i64, (reponame.clone(),));
                            }

                            Some(ChangesetMessage::Changeset((hg_cs, bcs))) if encountered_error.is_none() => {
                                current_batch.push((hg_cs, bcs));
                            }

                            Some(ChangesetMessage::ChangesetDone(sender)) if encountered_error.is_none() => {
                                pending_messages.push_back(sender);
                            }

                            Some(ChangesetMessage::Log((_, lag))) if encountered_error.is_none() => {
                                pending_log.push_back(lag);
                            }

                            Some(ChangesetMessage::ChangesetDone(sender)) =>{
                                let e = encountered_error.unwrap();
                                sender.send(Err(anyhow::anyhow!("Error processing changesets: {:?}", e))).await?;
                                return Err(e);
                            }

                            Some(ChangesetMessage::Log((_, _))) | Some(ChangesetMessage::Changeset(_)) => {}

                            None => break,
                        }

                        if current_batch.len() >= MAX_CHANGESET_BATCH_SIZE {
                            if let Err(e) = flush_batch(&changeset_es, &mut current_batch, &mut pending_messages, &mut pending_log, &changeset_logger, reponame.clone()).await {
                                return Err(anyhow::anyhow!("Error processing changesets: {:?}", e));
                            }

                        }
                    }
                    _ = flush_timer.tick() => {
                            if let Err(e) = flush_batch(&changeset_es, &mut current_batch, &mut pending_messages, &mut pending_log, &changeset_logger, reponame.clone()).await {
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
                    }
                    let elapsed = start.elapsed().as_secs() / batch_size as u64;
                    STATS::changeset_upload_time_s.add_value(elapsed as i64, (reponame.clone(),));
                }

                while let Some(Some(lag)) = pending_log.pop_front() {
                    STATS::synced_commits.add_value(1, (reponame.clone(),));
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

    pub async fn send_content(&mut self, content_msg: ContentMessage) -> Result<()> {
        self.content_sender
            .send(content_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_file_or_tree(&mut self, ft_msg: FileOrTreeMessage) -> Result<()> {
        self.files_and_trees_sender
            .send(ft_msg)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_changeset(&mut self, cs_msg: ChangesetMessage) -> Result<()> {
        self.changeset_sender
            .send(cs_msg)
            .await
            .map_err(|err| err.into())
    }
}
