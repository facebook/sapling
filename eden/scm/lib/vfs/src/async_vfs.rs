/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::thread;
use std::thread::JoinHandle;

use anyhow::Error;
use anyhow::Result;
use blob::Blob;
use slex::Items;
use slex::Work as SlexWork;
use slex::WorkOptions;
use slex::WorkScope;
use slex::WorkShape;
use tokio::sync::oneshot;
use types::RepoPath;
use types::RepoPathBuf;

use crate::RemoveOptions;
use crate::UpdateFlag;
use crate::VFS;

pub struct AsyncVfsWriter {
    sender: Option<flume::Sender<WorkItem>>,
    handles: Vec<JoinHandle<()>>,
}

struct WorkItem {
    res: oneshot::Sender<Result<ActionResult>>,
    action: Work,
}

#[derive(Debug)]
pub enum Work {
    Write(RepoPathBuf, Blob, UpdateFlag, Option<UpdateFlag>),
    Remove(RepoPathBuf),
    SetExecutable(RepoPathBuf, bool),
    Batch(Vec<Work>),
}

#[derive(Debug, thiserror::Error)]
pub enum VfsBatchError {
    /// A specific VFS operation failed; callers can use the `Work` to classify by action/path.
    #[error("VFS operation failed for {0:?}: {1}")]
    Work(Work, Error),
    /// The producer or batch finalizer failed outside a single VFS operation.
    #[error("VFS batch failed: {0}")]
    Batch(Error),
}

impl VfsBatchError {
    pub fn into_error(self) -> Error {
        match self {
            Self::Work(_, err) | Self::Batch(err) => err,
        }
    }
}

/// VFS batch results use the same item type as input: successful results echo completed work.
pub type VfsBatchItems = Items<Work, VfsBatchError>;

const VFS_WORK_BATCH_ITEMS: usize = 32;

impl Work {
    pub fn path(&self) -> &RepoPath {
        match self {
            Self::Write(path, ..) => path,
            Self::Remove(path) => path,
            Self::SetExecutable(path, ..) => path,
            Self::Batch(_) => panic!("Work::Batch has no single path"),
        }
    }
}

/// Async write interface to `VFS`.
/// Creating `AsyncVfsWriter` spawns worker threads that handle load internally.
/// If the future returned by `AsyncVfsWriter` functions is dropped, it's corresponding job may be dropped from the queue without executing.
/// Drop handler for `AsyncVfsWriter` blocks until underlyning threads terminate.
impl AsyncVfsWriter {
    pub fn spawn_new(vfs: VFS, workers: usize) -> Self {
        let (sender, receiver) = flume::unbounded();
        let sender = Some(sender);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let receiver = receiver.clone();
            let vfs = vfs.clone();
            handles.push(thread::spawn(move || async_vfs_worker(vfs, receiver)));
        }
        Self { sender, handles }
    }

    pub async fn write<B: Into<Blob>>(
        &self,
        path: RepoPathBuf,
        data: B,
        flag: UpdateFlag,
    ) -> Result<usize> {
        self.submit_action(Work::Write(path, data.into(), flag, None))
            .await
            .map(|r| r.bytes_written)
    }

    pub async fn write_batch<B: Into<Blob>>(
        &self,
        batch: impl IntoIterator<Item = (RepoPathBuf, B, UpdateFlag)>,
    ) -> Result<usize> {
        let batch = batch
            .into_iter()
            .map(|(path, data, flag)| Work::Write(path, data.into(), flag, None))
            .collect();
        self.submit_action(Work::Batch(batch))
            .await
            .map(|r| r.bytes_written)
    }

    pub async fn remove_batch(&self, batch: Vec<RepoPathBuf>) -> Result<Vec<(RepoPathBuf, Error)>> {
        let batch = batch.into_iter().map(Work::Remove).collect();
        self.submit_action(Work::Batch(batch))
            .await
            .map(|r| r.remove_errors)
    }

    pub async fn set_executable(&self, path: RepoPathBuf, flag: bool) -> Result<()> {
        self.submit_action(Work::SetExecutable(path, flag))
            .await
            .map(|_| ())
    }

    async fn submit_action(&self, action: Work) -> Result<ActionResult> {
        let (tx, rx) = oneshot::channel();
        let wi = WorkItem { action, res: tx };
        let _ = self.sender.as_ref().unwrap().send(wi);
        rx.await?
    }
}

struct ActionResult {
    bytes_written: usize,
    remove_errors: Vec<(RepoPathBuf, Error)>,
}

fn remove_options() -> RemoveOptions {
    RemoveOptions::IGNORE_MISSING_PATH
        | RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK
        | RemoveOptions::PRUNE_EMPTY_PARENTS
}

fn async_vfs_worker(vfs: VFS, receiver: flume::Receiver<WorkItem>) {
    for item in receiver {
        // Quickcheck - if caller future dropped while item was in queue, no reason to execute
        // One use case for this - if calling stream in checkout encounters an error, the stream is dropped
        // However some items are still in queue - we should not execute them at this point
        if item.res.is_closed() {
            continue;
        }
        let result = execute_action(&vfs, item.action);
        let _ = item.res.send(result);
    }
}

fn execute_action(vfs: &VFS, action: Work) -> Result<ActionResult> {
    let mut bytes_written = 0;
    let mut remove_errors = Vec::new();

    match action {
        Work::Write(path, data, flag, from_flag) => {
            if matches!(from_flag, Some(UpdateFlag::Symlink)) {
                vfs.rewrite_symlink(&path, data, flag)?;
            } else {
                bytes_written += vfs.write(&path, data, flag)?;
            }
        }
        Work::Remove(path) => {
            if let Err(err) = vfs.remove(&path, remove_options()) {
                remove_errors.push((path, err));
            }
        }
        Work::SetExecutable(path, flag) => vfs.set_executable(&path, flag)?,
        Work::Batch(batch) => {
            for action in batch {
                let res = execute_action(vfs, action)?;
                bytes_written += res.bytes_written;
                remove_errors.extend(res.remove_errors);
            }
        }
    }

    Ok(ActionResult {
        bytes_written,
        remove_errors,
    })
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

impl VFS {
    /// Process VFS work with `slex`.
    ///
    /// Per-work failures are reported as `Items` errors with the failed work attached. Producer
    /// and finalization errors use the same error channel without an attached work item.
    pub fn batch_items(&self, workers: usize, input: VfsBatchItems) -> VfsBatchItems {
        let vfs = self.clone();
        SlexWork::run(
            WorkOptions::new()
                .max_workers(workers)
                .inline_items(1)
                .work_chunk_items(VFS_WORK_BATCH_ITEMS)
                .result_batch_items(VFS_WORK_BATCH_ITEMS),
            input,
            WorkShape::batch_finalize(
                Vec::<RepoPathBuf>::new,
                move |batch, scope| match batch {
                    Ok(batch) => execute_batch_work(&vfs, batch, scope),
                    Err(err) => {
                        scope.send_error(err);
                        Ok(())
                    }
                },
                {
                    let vfs = self.clone();
                    move |locals| finish_batch_work(&vfs, locals)
                },
            ),
        )
    }

    /// Open a batch writing session with bounded channels.
    ///
    /// Returns:
    /// - `work_sender`: Send `Work` items to be processed by worker threads
    /// - `result_receiver`: Receive results from worker threads. `Ok(work)` for success,
    ///   `Err(VfsBatchError::Work(work, error))` for per-work failure, or
    ///   `Err(VfsBatchError::Batch(error))` for batch-level errors.
    ///
    /// The work channel is bounded to `queue_size` to limit memory usage.
    ///
    /// Workers exit when the work sender is dropped (channel closes).
    /// The result receiver closes when all workers have exited.
    ///
    /// On Windows, workers track symlink writes and call `update_symlinks` after
    /// all workers have finished processing.
    pub fn batch(
        &self,
        workers: usize,
        queue_size: usize,
    ) -> (
        flume::Sender<Work>,
        flume::Receiver<Result<Work, (Option<Work>, Error)>>,
    ) {
        let (work_tx, work_rx) = flume::bounded::<Work>(queue_size);
        let (result_tx, result_rx) = flume::unbounded();

        // Channel for worker synchronization. Workers drop their sender when done,
        // then wait for the receiver to close (all senders dropped).
        // This handles panics gracefully since the sender is dropped during unwinding.
        let (done_tx, done_rx) = flume::unbounded::<()>();

        for _ in 0..workers {
            let vfs = self.clone();
            let work_rx = work_rx.clone();
            let result_tx = result_tx.clone();
            let done_tx = done_tx.clone();
            let done_rx = done_rx.clone();
            thread::spawn(move || {
                batch_worker(&vfs, work_rx, result_tx, done_tx, done_rx);
            });
        }

        // Drop the original done_tx so only worker clones remain.
        drop(done_tx);

        (work_tx, result_rx)
    }
}

fn execute_batch_work(
    vfs: &VFS,
    batch: Vec<Work>,
    scope: &mut WorkScope<'_, Work, Work, VfsBatchError, Vec<RepoPathBuf>>,
) -> Result<(), VfsBatchError> {
    for work in batch {
        if let Work::Write(path, _, UpdateFlag::Symlink, _) = &work {
            scope.local_mut().push(path.clone());
        }

        match execute_batch_item(vfs, work) {
            Ok(work) => {
                if !scope.send_result([work]) {
                    return Ok(());
                }
            }
            Err(err) => {
                if !scope.send_error(err) {
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}

fn execute_batch_item(vfs: &VFS, work: Work) -> Result<Work, VfsBatchError> {
    let result = match &work {
        Work::Write(path, data, flag, from_flag) => {
            if matches!(from_flag, Some(UpdateFlag::Symlink)) {
                vfs.rewrite_symlink(path, data.clone(), *flag).map(|_| ())
            } else {
                vfs.write(path, data.clone(), *flag).map(|_| ())
            }
        }
        Work::SetExecutable(path, exec) => vfs.set_executable(path, *exec).map(|_| ()),
        Work::Remove(path) => vfs.remove(path, remove_options()),
        // Don't support batch for now - doesn't really make sense anyway since it precludes
        // parallelism.
        Work::Batch(_) => Err(anyhow::anyhow!("Work::Batch not supported")),
    };

    match result {
        Ok(()) => Ok(work),
        Err(err) => Err(VfsBatchError::Work(work, err)),
    }
}

fn finish_batch_work(
    vfs: &VFS,
    locals: Vec<Vec<RepoPathBuf>>,
) -> Result<Option<Vec<Work>>, VfsBatchError> {
    #[cfg(windows)]
    {
        let local_symlinks: Vec<_> = locals.into_iter().flatten().collect();
        if vfs.supports_symlinks() && !local_symlinks.is_empty() {
            let path_refs: Vec<&types::RepoPath> =
                local_symlinks.iter().map(|p| p.as_ref()).collect();
            vfs.reconcile_symlinks(&path_refs)
                .map_err(VfsBatchError::Batch)?;
        }
        Ok(None)
    }

    #[cfg(not(windows))]
    {
        let _ = (vfs, locals);
        Ok(None)
    }
}

fn batch_worker(
    vfs: &VFS,
    work_rx: flume::Receiver<Work>,
    result_tx: flume::Sender<Result<Work, (Option<Work>, Error)>>,
    done_tx: flume::Sender<()>,
    done_rx: flume::Receiver<()>,
) {
    // Track symlinks locally for this worker
    let mut local_symlinks: Vec<RepoPathBuf> = Vec::new();

    while let Ok(work) = work_rx.recv() {
        // Track symlink writes for Windows symlink fixing
        if let Work::Write(path, _, UpdateFlag::Symlink, _) = &work {
            local_symlinks.push(path.clone());
        }

        if result_tx
            .send(legacy_batch_result(execute_batch_item(vfs, work)))
            .is_err()
        {
            break;
        }
    }

    // Signal we're done processing work. When all workers have dropped their
    // done_tx (including due to panic), done_rx will close.
    drop(done_tx);

    // Wait for all workers to finish (blocks until done_rx closes).
    let _ = done_rx.recv();

    // Now run symlink fixes on Windows (each worker processes its own symlinks)
    #[cfg(windows)]
    {
        if vfs.supports_symlinks() && !local_symlinks.is_empty() {
            let path_refs: Vec<&types::RepoPath> =
                local_symlinks.iter().map(|p| p.as_ref()).collect();
            if let Err(e) = vfs.reconcile_symlinks(&path_refs) {
                let _ = result_tx.send(Err((None, e)));
            }
        }
    }
}

fn legacy_batch_result(result: Result<Work, VfsBatchError>) -> Result<Work, (Option<Work>, Error)> {
    match result {
        Ok(work) => Ok(work),
        Err(VfsBatchError::Work(work, err)) => Err((Some(work), err)),
        Err(VfsBatchError::Batch(err)) => Err((None, err)),
    }
}
