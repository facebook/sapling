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

    /// Iterate through all collected backtraces. Most recent call last.
    pub fn iter(&self) -> BacktraceIter<'_> {
        BacktraceIter {
            collector: self,
            pos: 0,
            current_backtrace: Vec::new(),
        }
    }
}

/// Iterator over collected backtraces.
pub struct BacktraceIter<'a> {
    collector: &'a BacktraceCollector,
    /// Current position in traces.
    pos: usize,
    /// Current backtrace (indices into names). Most recent call last.
    current_backtrace: Vec<usize>,
}

impl<'a> Iterator for BacktraceIter<'a> {
    type Item = Vec<&'a str>;

    /// Get the next backtrace. Most recent call last.
    fn next(&mut self) -> Option<Self::Item> {
        let first_id: TypedId = (*self.collector.traces.get(self.pos)?).into();
        self.pos += 1;

        let reuse_count = match first_id {
            TypedId::Reuse(n) => n,
            TypedId::Frame(_) => return None,
        };

        self.current_backtrace.truncate(reuse_count);
        while let Some(&id) = self.collector.traces.get(self.pos) {
            let id: TypedId = id.into();
            match id {
                TypedId::Reuse(_) => break,
                TypedId::Frame(idx) => {
                    self.current_backtrace.push(idx);
                    self.pos += 1;
                }
            }
        }

        let names = self
            .current_backtrace
            .iter()
            .filter_map(|&idx| self.collector.names.get_index(idx).map(|s| s.as_str()))
            .collect();
        Some(names)
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    fn check_round_trip(backtraces: Vec<Vec<String>>) -> bool {
        let mut collector = BacktraceCollector::default();
        for backtrace in &backtraces {
            collector.push_backtrace(backtrace.clone());
        }
        let backtraces2: Vec<_> = collector.iter().collect();
        backtraces == backtraces2
    }

    quickcheck! {
        fn test_round_trip(backtraces: Vec<Vec<String>>) -> bool {
            check_round_trip(backtraces)
        }
    }
}
