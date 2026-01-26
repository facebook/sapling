/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use ascii_tree::AsciiOptions;
use ascii_tree::DescribeTreeSpan;
use ascii_tree::Tree;
use ascii_tree::TreeSpan;
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
            .take_while(|&(name, idx)| {
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

    /// Convert backtraces into a tree structure.
    pub fn tree(&self) -> Tree<&str> {
        let mut tree = Tree::default();
        let mut current_path: Vec<usize> = Vec::new(); // Tree indexes for the current backtrace path
        let mut start_time = 0;
        for &id in &self.traces {
            match id.into() {
                TypedId::Reuse(n) => {
                    current_path.truncate(n);
                    for &node_id in &current_path {
                        tree.0[node_id].duration += 1;
                    }
                    start_time += 1;
                }
                TypedId::Frame(idx) => {
                    let frame_name = self.names.get_index(idx).map(|s| s.as_str()).unwrap();
                    let parent_id = current_path.last().copied().unwrap_or(0);
                    let tree_span = TreeSpan {
                        start_time,
                        duration: 1,
                        extra: Some(frame_name),
                        ..Default::default()
                    };
                    let tree_span_id = tree.push(parent_id, tree_span);
                    current_path.push(tree_span_id);
                }
            }
        }
        tree
    }

    /// Render backtraces as an ASCII summary.
    pub fn ascii_summary(&self) -> String {
        let mut tree = self.tree();
        let opts = AsciiOptions {
            min_duration_to_hide: 1,
            ..Default::default()
        };
        tree.merge_children(&opts, &|t| t.extra);

        struct Desc;
        impl DescribeTreeSpan<&str> for Desc {
            fn name(&self, span: &TreeSpan<&str>) -> String {
                let name = span.extra.unwrap_or_default();
                match name.rsplit_once(" at ") {
                    Some(v) => v.0.to_string(),
                    None => name.to_string(),
                }
            }
            fn source(&self, span: &TreeSpan<&str>) -> String {
                let name = span.extra.unwrap_or_default();
                match name.rsplit_once(" at ") {
                    Some(v) => v.1.to_string(),
                    None => String::new(),
                }
            }
            fn duration_title(&self) -> String {
                "Dur".to_string()
            }
            fn call_count(&self, span: &TreeSpan<&str>) -> String {
                if span.call_count > 1 {
                    format!(" ({})", span.call_count)
                } else {
                    String::new()
                }
            }
        }

        let rows = tree.render_ascii_rows(&opts, &Desc);
        rows.to_string()
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

    #[test]
    fn test_basic_ascii_summary() {
        let mut collector = BacktraceCollector::default();
        for names in [
            &["_start", "main", "fib"][..],
            &["_start", "main", "fib", "fib1 at a.py:12"],
            &["_start", "main", "fib", "fib2 at a.py:22"],
            &["_start", "main", "output"],
        ] {
            let names = names.iter().map(ToString::to_string).collect::<Vec<_>>();
            collector.push_backtrace(names);
        }
        let out = format!("\n{}", collector.ascii_summary());
        assert_eq!(
            out,
            r#"
Start  Dur | Name               Source
    1   +4 | _start            
    1   +4 | main              
    1   +3  \ fib              
    2   +1   \ fib1             a.py:12
    3   +1   \ fib2             a.py:22
    4   +1  \ output           
"#
        );
    }
}
