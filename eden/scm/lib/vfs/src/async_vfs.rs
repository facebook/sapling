/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::thread;
use std::thread::JoinHandle;

use anyhow::Result;
use crossbeam::channel;
use crossbeam::channel::Receiver;
use crossbeam::channel::Sender;
use minibytes::Bytes;
use tokio::sync::oneshot;
use types::RepoPathBuf;

use crate::UpdateFlag;
use crate::VFS;

pub struct AsyncVfsWriter {
    sender: Option<Sender<WorkItem>>,
    handles: Vec<JoinHandle<()>>,
}

struct WorkItem {
    res: oneshot::Sender<Result<usize>>,
    action: Action,
}
#[derive(Debug)]
enum Action {
    Write(RepoPathBuf, Bytes, UpdateFlag),
    Remove(RepoPathBuf),
    SetExecutable(RepoPathBuf, bool),
    Batch(Vec<Action>),
}

/// Async write interface to `VFS`.
/// Creating `AsyncVfsWriter` spawns worker threads that handle load internally.
/// If the future returned by `AsyncVfsWriter` functions is dropped, it's corresponding job may be dropped from the queue without executing.
/// Drop handler for `AsyncVfsWriter` blocks until underlyning threads terminate.
impl AsyncVfsWriter {
    pub fn spawn_new(vfs: VFS, workers: usize) -> Self {
        let (sender, receiver) = channel::unbounded();
        let sender = Some(sender);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let receiver = receiver.clone();
            let vfs = vfs.clone();
            handles.push(thread::spawn(move || async_vfs_worker(vfs, receiver)));
        }
        Self { sender, handles }
    }

    pub async fn write<B: Into<Bytes>>(
        &self,
        path: RepoPathBuf,
        data: B,
        flag: UpdateFlag,
    ) -> Result<usize> {
        self.submit_action(Action::Write(path, data.into(), flag))
            .await
    }

    pub async fn write_batch<B: Into<Bytes>>(
        &self,
        batch: impl IntoIterator<Item = (RepoPathBuf, B, UpdateFlag)>,
    ) -> Result<usize> {
        let batch = batch
            .into_iter()
            .map(|(path, data, flag)| Action::Write(path, data.into(), flag))
            .collect();
        self.submit_action(Action::Batch(batch)).await
    }

    pub async fn remove(&self, path: RepoPathBuf) -> Result<()> {
        self.submit_action(Action::Remove(path)).await.map(|_| ())
    }

    pub async fn remove_batch(&self, batch: Vec<RepoPathBuf>) -> Result<()> {
        let batch = batch.into_iter().map(Action::Remove).collect();
        self.submit_action(Action::Batch(batch)).await.map(|_| ())
    }

    pub async fn set_executable(&self, path: RepoPathBuf, flag: bool) -> Result<()> {
        self.submit_action(Action::SetExecutable(path, flag))
            .await
            .map(|_| ())
    }

    async fn submit_action(&self, action: Action) -> Result<usize> {
        let (tx, rx) = oneshot::channel();
        let wi = WorkItem { action, res: tx };
        self.sender.as_ref().unwrap().send(wi).ok();
        rx.await?
    }
}

fn async_vfs_worker(vfs: VFS, receiver: Receiver<WorkItem>) {
    for item in receiver {
        // Quickcheck - if caller future dropped while item was in queue, no reason to execute
        // One use case for this - if calling stream in checkout encounters an error, the stream is dropped
        // However some items are still in queue - we should not execute them at this point
        if item.res.is_closed() {
            continue;
        }
        let result = execute_action(&vfs, item.action);
        item.res.send(result).ok();
    }
}

fn execute_action(vfs: &VFS, action: Action) -> Result<usize> {
    match action {
        Action::Write(path, data, flag) => vfs.write(&path, &data, flag),
        Action::Remove(path) => vfs.remove(&path).map(|_| 0),
        Action::SetExecutable(path, flag) => vfs.set_executable(&path, flag).map(|_| 0),
        Action::Batch(batch) => {
            let mut total = 0;
            for action in batch {
                total += execute_action(vfs, action)?;
            }
            Ok(total)
        }
    }
}

impl Drop for AsyncVfsWriter {
    // Good citizen behavior - waiting until threads stop when AsyncVfs is dropped
    // This also will propagate panic from a worker thread into caller
    fn drop(&mut self) {
        self.sender.take();
        for handle in self.handles.drain(..) {
            handle.join().unwrap();
        }
    }
}
