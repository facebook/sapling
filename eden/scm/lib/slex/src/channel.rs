/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Bounded blocking result channels used by executor streaming APIs.

pub use crossfire::RecvError;
pub use crossfire::RecvTimeoutError;
pub use crossfire::SendError;
pub use crossfire::TryRecvError;
pub use crossfire::TrySendError;

type InnerSender<T> = crossfire::MTx<crossfire::mpsc::Array<T>>;
type InnerReceiver<T> = crossfire::Rx<crossfire::mpsc::Array<T>>;

/// Multi-producer sender for bounded executor result queues.
pub struct Sender<T: Send + 'static> {
    inner: InnerSender<T>,
}

impl<T: Send + 'static> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Send + 'static> Sender<T> {
    /// Send an item, blocking while the bounded queue is full.
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.inner.send(item)
    }

    /// Try to send an item without blocking.
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.inner.try_send(item)
    }

    /// Whether all receivers have disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.inner.is_disconnected()
    }
}

/// Single-consumer receiver for bounded executor result queues.
pub struct Receiver<T: Send + 'static> {
    inner: InnerReceiver<T>,
}

impl<T: Send + 'static> Receiver<T> {
    /// Receive one item, blocking while the queue is empty.
    pub fn recv(&self) -> Result<T, RecvError> {
        self.inner.recv()
    }

    /// Try to receive one item without blocking.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner.try_recv()
    }

    /// Whether all senders have disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.inner.is_disconnected()
    }
}

/// Create a bounded blocking MPSC channel.
///
/// Crossfire treats capacity 0 as 1. This wrapper preserves that behavior.
pub fn bounded<T: Send + 'static>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = crossfire::mpsc::bounded_blocking(capacity);
    (Sender { inner: sender }, Receiver { inner: receiver })
}

/// Blocking iterator over a [`Receiver`].
pub struct ReceiverIter<T: Send + 'static> {
    receiver: Receiver<T>,
}

impl<T: Send + 'static> Iterator for ReceiverIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

impl<T: Send + 'static> IntoIterator for Receiver<T> {
    type IntoIter = ReceiverIter<T>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        ReceiverIter { receiver: self }
    }
}
