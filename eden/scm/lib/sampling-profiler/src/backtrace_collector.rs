/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use indexmap::IndexSet;

#[derive(Default)]
pub struct BacktraceCollector {
    /// Frame names.
    names: IndexSet<String>,

    /// See `Id` for details.
    /// This is a flat `Vec` instead of `Vec`s of `Vec`s as an attempt to reduce
    /// memory fragmentation.
    traces: Vec<Id>,

    /// (Cache) the frame names in the last backtrace.
    /// Most recent call last.
    last_backtrace: Vec<usize>,
}

/// Tagged integer:
/// - (n << 1) | 1: A new backtrace, reusing `n` top frames from the previous backtrace.
/// - (n << 1): A frame, with name `n`.
#[derive(Copy, Clone)]
struct Id(usize);

#[derive(Copy, Clone)]
enum TypedId {
    Reuse(usize),
    Frame(usize),
}

impl From<Id> for TypedId {
    fn from(value: Id) -> Self {
        if value.0 & 1 == 0 {
            Self::Frame(value.0 >> 1)
        } else {
            Self::Reuse(value.0 >> 1)
        }
    }
}

impl From<TypedId> for Id {
    fn from(value: TypedId) -> Self {
        match value {
            TypedId::Reuse(n) => Self((n << 1) | 1),
            TypedId::Frame(n) => Self(n << 1),
        }
    }
}

impl BacktraceCollector {
    /// Push a backtrace. Most recent call last.
    pub fn push_backtrace(&mut self, frame_names: Vec<String>) {
        // Find how many frames from the top (beginning) match the last backtrace.
        let reuse_count = frame_names
            .iter()
            .zip(self.last_backtrace.iter())
            .take_while(|&(ref name, idx)| {
                self.names
                    .get_index(*idx)
                    .map(|s| s.as_str() == name.as_str())
                    .unwrap_or(false)
            })
            .count();

        // Push the reuse marker to indicate a new backtrace.
        self.traces.push(TypedId::Reuse(reuse_count).into());

        // Truncate last_backtrace to the reused portion.
        self.last_backtrace.truncate(reuse_count);

        // Push new frames and update the cache.
        for name in frame_names.into_iter().skip(reuse_count) {
            let (idx, _) = self.names.insert_full(name);
            self.traces.push(TypedId::Frame(idx).into());
            self.last_backtrace.push(idx);
        }
    }
}
