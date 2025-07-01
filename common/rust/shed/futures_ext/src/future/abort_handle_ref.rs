/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::future::Future;
use std::sync::Arc;

use futures::future::AbortHandle;
use futures::future::abortable;

/// Spawns a new task returning an abort handle for it.
///
/// It is similar to [tokio::task::spawn] but instad of returning a JoinHandle it will return
/// an [ControlledHandle]. The [ControlledHandle] can be used to directly abort the task that was
/// spawned. [ControlledHandle] can be cloned resulting in a new handle to the same underlying
/// task. Dropping all [ControlledHandle] instances pointing to a given task will result in the
/// abort of that task.
///
/// The use case this function is tasks that "run in the background" and are tied to a specific
/// object. We attach the [ControlledHandle] to the object in question so that the background task is
/// "dropped" (aborted) when the object is dropped.
pub fn spawn_controlled<T>(t: T) -> ControlledHandle
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let (abortable_future, abort_handle) = abortable(t);
    tokio::task::spawn(abortable_future);
    ControlledHandle::new(abort_handle)
}

/// A handle that can abort the spawned task that it is associated with aborted. The underlying
/// task also gets aborted when there are no more handles referencing it.
#[derive(Clone, Debug)]
pub struct ControlledHandle(
    #[allow(dead_code)] // Used for Inner's Drop implementation.
    Arc<Inner>,
);

impl ControlledHandle {
    fn new(abort_handle: AbortHandle) -> Self {
        Self(Arc::new(Inner(abort_handle)))
    }
    // There's probably nothing wrong with adding an explicit abort function but we don't need it
    // right now.
}

#[derive(Debug)]
struct Inner(AbortHandle);

impl Drop for Inner {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    fn handle_and_counting_receiver() -> (ControlledHandle, mpsc::Receiver<u64>) {
        let (tx, rx) = mpsc::channel(1);
        let handle = spawn_controlled(async move {
            let mut x: u64 = 0;
            loop {
                tx.send(x).await.unwrap();
                x += 1;
            }
        });
        (handle, rx)
    }

    #[tokio::test]
    async fn test_no_handles_abort() {
        let (handle, mut rx) = handle_and_counting_receiver();
        assert_eq!(rx.recv().await, Some(0));
        {
            let _ = handle.clone();
        }
        assert_eq!(rx.recv().await, Some(1));
        drop(handle);
        assert_eq!(rx.recv().await, None);
    }
}
