/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use crate::AsciiOptions;

pub struct TreeSpan<T> {
    pub children: Vec<usize>,

    pub start_time: u64,
    pub duration: u64,
    pub call_count: usize,

    /// Extra data associated to this `TreeSpan`.
    /// `None` indicates a root `TreeSpan`.
    pub extra: Option<T>,
}

impl<T: Default> Default for TreeSpan<T> {
    fn default() -> Self {
        Self {
            children: Default::default(),
            start_time: Default::default(),
            duration: Default::default(),
            call_count: 1,
            extra: Default::default(),
        }
    }
}

impl<T> TreeSpan<T> {
    /// Whether the current [`RawTreeSpan`] covers another [`RawTreeSpan`] timestamp-wise.
    pub fn covers(&self, other: &Self) -> bool {
        if self.is_incomplete() {
            self.start_time <= other.start_time
        } else {
            self.end_time() >= other.end_time() && self.start_time <= other.start_time
        }
    }

    /// End time (inaccurate if this is a merged span, i.e. call_count > 1).
    pub fn end_time(&self) -> u64 {
        self.start_time + self.duration
    }

    pub fn is_root(&self) -> bool {
        self.extra.is_none()
    }

    /// Is this span considered interesting (should it be printed)?
    pub fn is_interesting(&self, opts: &AsciiOptions, parent: Option<&Self>) -> bool {
        if self.call_count == 0 {
            return false;
        }

        if let Some(parent) = parent {
            // Special case: Parent is root (which does not have a duration). Show the span.
            if parent.is_root() {
                return true;
            }

            // "to_show" conditions
            if opts.min_duration_parent_percentage_to_show != 0 && parent.is_interesting(opts, None)
            {
                if self.duration
                    >= (parent.duration * opts.min_duration_parent_percentage_to_show as u64) / 100
                {
                    return true;
                }
            }

            // "to_hide" conditions
            if self.duration
                < (parent.duration * opts.min_duration_parent_percentage_to_hide as u64) / 100
            {
                return false;
            }
        }

        // "to_hide" conditions
        self.duration >= opts.min_duration_to_hide
    }

    /// A very long, impractical `duration` that indicates an incomplete span
    /// that has started but not ended.
    pub const fn incomplete_duration() -> u64 {
        1 << 63
    }

    pub fn is_incomplete(&self) -> bool {
        self.duration >= Self::incomplete_duration()
    }
}
