/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::convert::Infallible;

use smallvec::SmallVec;

use crate::Items;
use crate::channel;

const DEFAULT_BATCH_ITEMS: usize = 128;
const DEFAULT_QUEUE_SIZE: usize = 32;
const MAX_INITIAL_BUFFER_CAPACITY: usize = 128;

/// Options for writing item batches into an [`Items`] transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemsWriterOptions {
    /// Number of items to accumulate before flushing one stream batch.
    pub batch_items: usize,
    /// Number of batches to buffer for streaming results.
    ///
    /// With the default batch size this buffers roughly 4k items, or four batches per default
    /// downstream worker.
    pub queue_size: usize,
}

impl ItemsWriterOptions {
    /// Start from default writer options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the stream batch size.
    pub fn batch_items(mut self, batch_items: usize) -> Self {
        self.batch_items = batch_items.max(1);
        self
    }

    /// Set the bounded stream queue size in batches.
    pub fn queue_size(mut self, queue_size: usize) -> Self {
        self.queue_size = queue_size.max(1);
        self
    }
}

impl Default for ItemsWriterOptions {
    fn default() -> Self {
        Self {
            batch_items: DEFAULT_BATCH_ITEMS,
            queue_size: DEFAULT_QUEUE_SIZE,
        }
    }
}

enum ItemsWriterBackend<T: Send + 'static, E: Send + 'static> {
    Inline,
    Stream(channel::Sender<Result<Vec<T>, E>>),
}

/// Producer side for building `Items<T, E>`.
///
/// The writer accumulates success batches and error events in order. Inline writers use those
/// events as the final ready result. Streaming writers flush those events to a crossfire-backed
/// channel.
pub struct ItemsWriter<T: Send + 'static, E: Send + 'static = Infallible> {
    output_events: SmallVec<[Result<Vec<T>, E>; 1]>,
    batch_items: usize,
    disconnected: bool,
    backend: ItemsWriterBackend<T, E>,
}

impl<T: Send + 'static, E: Send + 'static> ItemsWriter<T, E> {
    /// Run a producer inline or on the blocking executor and return its `Items`.
    pub fn from_process(
        should_spawn: bool,
        process: impl FnOnce(&mut Self) + Send + 'static,
    ) -> Items<T, E> {
        Self::from_process_with_options(should_spawn, ItemsWriterOptions::default(), process)
    }

    /// Run a producer inline or on the blocking executor with explicit writer options.
    pub fn from_process_with_options(
        should_spawn: bool,
        options: ItemsWriterOptions,
        process: impl FnOnce(&mut Self) + Send + 'static,
    ) -> Items<T, E> {
        if should_spawn {
            let (mut writer, items): (Self, Items<T, E>) = Self::stream_with_options(options);
            std::mem::drop(async_runtime::spawn_blocking(move || {
                process(&mut writer);
            }));
            items
        } else {
            let mut writer = Self::inline_with_options(options);
            process(&mut writer);
            writer.finish()
        }
    }

    /// Create an inline writer.
    pub fn inline() -> Self {
        Self::inline_with_options(ItemsWriterOptions::default())
    }

    /// Create an inline writer with explicit options.
    pub fn inline_with_options(options: ItemsWriterOptions) -> Self {
        Self {
            output_events: SmallVec::new(),
            batch_items: options.batch_items,
            disconnected: false,
            backend: ItemsWriterBackend::Inline,
        }
    }

    /// Create a streaming writer and matching item stream.
    pub fn stream() -> (Self, Items<T, E>) {
        Self::stream_with_options(ItemsWriterOptions::default())
    }

    /// Create a streaming writer and matching item stream with explicit options.
    pub fn stream_with_options(options: ItemsWriterOptions) -> (Self, Items<T, E>) {
        let (sender, receiver) = channel::bounded(options.queue_size);
        let writer = Self {
            output_events: SmallVec::new(),
            batch_items: options.batch_items,
            disconnected: false,
            backend: ItemsWriterBackend::Stream(sender),
        };
        let items = Items::stream(receiver.into_iter());
        (writer, items)
    }

    /// Append one item, flushing automatically in streaming mode.
    ///
    /// Returns `false` if the downstream receiver disconnected.
    pub fn push_item(&mut self, item: T) -> bool {
        if self.disconnected {
            return false;
        }

        match self.output_events.last_mut() {
            Some(Ok(batch)) => batch.push(item),
            _ => {
                let mut batch = Vec::with_capacity(initial_buffer_capacity(self.batch_items));
                batch.push(item);
                self.output_events.push(Ok(batch));
            }
        }
        self.flush_if_needed()
    }

    /// Append one success batch, flushing automatically in streaming mode.
    ///
    /// Returns `false` if the downstream receiver disconnected.
    pub fn push_batch(&mut self, batch: Vec<T>) -> bool {
        if batch.is_empty() {
            return true;
        }
        if self.disconnected {
            return false;
        }

        match self.output_events.last_mut() {
            Some(Ok(last)) => last.extend(batch),
            _ => self.output_events.push(Ok(batch)),
        }
        self.flush_if_needed()
    }

    /// Append one error event, flushing pending success items first to preserve ordering.
    ///
    /// Returns `false` if the downstream receiver disconnected.
    pub fn push_error(&mut self, err: E) -> bool {
        if !self.flush() {
            return false;
        }

        match &self.backend {
            ItemsWriterBackend::Inline => {
                self.output_events.push(Err(err));
                true
            }
            ItemsWriterBackend::Stream(_) => {
                self.output_events.push(Err(err));
                self.flush()
            }
        }
    }

    /// Flush the current buffered batch in streaming mode.
    pub fn flush(&mut self) -> bool {
        self.flush_retain()
    }

    fn flush_retain(&mut self) -> bool {
        if self.output_events.is_empty() || self.disconnected {
            return !self.disconnected;
        }

        match &self.backend {
            ItemsWriterBackend::Inline => true,
            ItemsWriterBackend::Stream(sender) => {
                for event in self.output_events.drain(..) {
                    if sender.send(event).is_err() {
                        self.disconnected = true;
                        return false;
                    }
                }
                true
            }
        }
    }

    fn flush_final(&mut self) -> bool {
        self.flush_retain()
    }

    /// Finish an inline writer and return the accumulated ready events.
    pub fn finish(mut self) -> Items<T, E> {
        let _ = self.flush_final();
        Items::Ready(
            std::mem::take(&mut self.output_events)
                .into_iter()
                .map(|event| event.map(Into::into))
                .collect(),
        )
    }

    /// Flush any remaining stream batch and close the writer.
    pub fn close(mut self) -> bool {
        self.flush_final()
    }

    fn flush_if_needed(&mut self) -> bool {
        match &self.backend {
            ItemsWriterBackend::Inline => true,
            ItemsWriterBackend::Stream(_) => {
                if self.current_success_len() >= self.batch_items {
                    self.flush()
                } else {
                    true
                }
            }
        }
    }

    fn current_success_len(&self) -> usize {
        match self.output_events.last() {
            Some(Ok(batch)) => batch.len(),
            _ => 0,
        }
    }

    pub(crate) fn take_events(&mut self) -> SmallVec<[Result<Vec<T>, E>; 1]> {
        std::mem::take(&mut self.output_events)
    }
}

impl<T: Send + 'static, E: Send + 'static> Drop for ItemsWriter<T, E> {
    fn drop(&mut self) {
        let _ = self.flush_final();
    }
}

impl<T: Send + 'static> ItemsWriter<T, Infallible> {
    /// Finish an infallible inline writer and return the accumulated ready items.
    pub fn finish_inline(self) -> Vec<T> {
        self.finish()
            .into_iter()
            .map(|item| match item {
                Ok(item) => item,
                Err(err) => match err {},
            })
            .collect()
    }
}

fn initial_buffer_capacity(batch_items: usize) -> usize {
    batch_items.min(MAX_INITIAL_BUFFER_CAPACITY)
}
