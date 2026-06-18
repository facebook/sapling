/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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

enum ItemsWriterBackend<T: Send + 'static> {
    Inline,
    Stream(channel::Sender<Vec<T>>),
}

/// Producer side for building `Items<T>`.
///
/// Each writer owns a local buffer. Inline writers use that buffer as the final ready result.
/// Streaming writers flush that buffer to a crossfire-backed channel in batches.
pub struct ItemsWriter<T: Send + 'static> {
    buffer: Vec<T>,
    batch_items: usize,
    disconnected: bool,
    backend: ItemsWriterBackend<T>,
}

impl<T: Send + 'static> ItemsWriter<T> {
    /// Run a producer inline or on the blocking executor and return its `Items`.
    pub fn from_process(
        should_spawn: bool,
        process: impl FnOnce(&mut Self) + Send + 'static,
    ) -> Items<T> {
        Self::from_process_with_options(should_spawn, ItemsWriterOptions::default(), process)
    }

    /// Run a producer inline or on the blocking executor with explicit writer options.
    pub fn from_process_with_options(
        should_spawn: bool,
        options: ItemsWriterOptions,
        process: impl FnOnce(&mut Self) + Send + 'static,
    ) -> Items<T> {
        if should_spawn {
            let (mut writer, items): (Self, Items<T>) = Self::stream_with_options(options);
            std::mem::drop(async_runtime::spawn_blocking(move || {
                process(&mut writer);
            }));
            items
        } else {
            let mut writer = Self::inline_with_options(options);
            process(&mut writer);
            Items::ready(writer.finish_inline())
        }
    }

    /// Create an inline writer.
    pub fn inline() -> Self {
        Self::inline_with_options(ItemsWriterOptions::default())
    }

    /// Create an inline writer with explicit options.
    pub fn inline_with_options(options: ItemsWriterOptions) -> Self {
        Self {
            buffer: Vec::with_capacity(initial_buffer_capacity(options.batch_items)),
            batch_items: options.batch_items,
            disconnected: false,
            backend: ItemsWriterBackend::Inline,
        }
    }

    /// Create a streaming writer and matching item stream.
    pub fn stream<E: Send + 'static>() -> (Self, Items<T, E>) {
        Self::stream_with_options(ItemsWriterOptions::default())
    }

    /// Create a streaming writer and matching item stream with explicit options.
    pub fn stream_with_options<E: Send + 'static>(
        options: ItemsWriterOptions,
    ) -> (Self, Items<T, E>) {
        let (sender, receiver) = channel::bounded(options.queue_size);
        let writer = Self {
            buffer: Vec::with_capacity(initial_buffer_capacity(options.batch_items)),
            batch_items: options.batch_items,
            disconnected: false,
            backend: ItemsWriterBackend::Stream(sender),
        };
        let items = Items::stream(receiver.into_iter().map(Ok));
        (writer, items)
    }

    /// Append one item, flushing automatically in streaming mode.
    ///
    /// Returns `false` if the downstream receiver disconnected.
    pub fn push_item(&mut self, item: T) -> bool {
        if self.disconnected {
            return false;
        }

        self.buffer.push(item);
        self.flush_if_needed()
    }

    /// Flush the current buffered batch in streaming mode.
    pub fn flush(&mut self) -> bool {
        self.flush_retain()
    }

    fn flush_retain(&mut self) -> bool {
        if self.buffer.is_empty() || self.disconnected {
            return !self.disconnected;
        }

        match &self.backend {
            ItemsWriterBackend::Inline => true,
            ItemsWriterBackend::Stream(sender) => {
                if sender.send(self.buffer.drain(..).collect()).is_err() {
                    self.disconnected = true;
                    false
                } else {
                    true
                }
            }
        }
    }

    fn flush_final(&mut self) -> bool {
        if self.buffer.is_empty() || self.disconnected {
            return !self.disconnected;
        }

        match &self.backend {
            ItemsWriterBackend::Inline => true,
            ItemsWriterBackend::Stream(sender) => {
                if sender.send(std::mem::take(&mut self.buffer)).is_err() {
                    self.disconnected = true;
                    false
                } else {
                    true
                }
            }
        }
    }

    /// Finish an inline writer and return the accumulated ready items.
    pub fn finish_inline(mut self) -> Vec<T> {
        std::mem::take(&mut self.buffer)
    }

    /// Flush any remaining stream batch and close the writer.
    pub fn close(mut self) -> bool {
        self.flush_final()
    }

    fn flush_if_needed(&mut self) -> bool {
        match &self.backend {
            ItemsWriterBackend::Inline => true,
            ItemsWriterBackend::Stream(_) => {
                if self.buffer.len() >= self.batch_items {
                    self.flush()
                } else {
                    true
                }
            }
        }
    }
}

impl<T: Send + 'static> Drop for ItemsWriter<T> {
    fn drop(&mut self) {
        let _ = self.flush_final();
    }
}

fn initial_buffer_capacity(batch_items: usize) -> usize {
    batch_items.min(MAX_INITIAL_BUFFER_CAPACITY)
}
