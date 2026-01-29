/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::hash::Hash;

use crate::AsciiOptions;
use crate::TreeSpan;
use crate::row::Alignment;
use crate::row::Row;
use crate::row::Rows;

#[derive(Clone)]
pub struct Tree<T>(pub Vec<TreeSpan<T>>);

impl<T> Default for Tree<T> {
    fn default() -> Self {
        Self(vec![TreeSpan {
            children: vec![],
            start_time: 0,
            duration: 0,
            call_count: 1,
            extra: None,
        }])
    }
}

/// Used by `render_ascii_rows` to get the actual columns from a `TreeSpan`.
pub trait DescribeTreeSpan<T> {
    fn name(&self, span: &TreeSpan<T>) -> String;
    fn source(&self, _span: &TreeSpan<T>) -> String {
        String::new()
    }
    fn start(&self, span: &TreeSpan<T>) -> String {
        span.start_time.to_string()
    }
    fn duration(&self, span: &TreeSpan<T>) -> String {
        format!("+{}", span.duration)
    }
    fn duration_title(&self) -> String {
        "Dur.ms".to_string()
    }
    fn call_count(&self, span: &TreeSpan<T>) -> String {
        if span.call_count > 1 {
            format!(" ({} times)", span.call_count)
        } else {
            assert!(span.call_count > 0);
            String::new()
        }
    }
    fn extra_metadata_lines(&self, _span: &TreeSpan<T>) -> Vec<String> {
        Vec::new()
    }
}

impl<T> Tree<T> {
    /// Push the given `TreeSpan` as a child of the parent `TreeSpan`.
    /// Return the index of the newly pushed `TreeSpan`.
    pub fn push(&mut self, parent_span_id: usize, span: TreeSpan<T>) -> usize {
        let id = self.0.len();
        self.0.push(span);
        self.0[parent_span_id].children.push(id);
        id
    }

    /// Merge multiple similar TreeSpans into one larger TreeSpan with
    /// a larger `call_count`.
    ///
    /// `extract_key` function extracts a `key` from a TreeSpan.
    /// TreeSpans with the same key might be merged.
    pub fn merge_children<K: Eq + Hash>(
        &mut self,
        opts: &AsciiOptions,
        extract_key: &impl Fn(&TreeSpan<T>) -> Option<K>,
    ) {
        // For example,
        //
        //   <root>
        //    |- span 1
        //    |   |- span 2
        //    |   |- span 3
        //    |   |- span 2
        //    |   |- span 3
        //    |   |- span 2
        //    |- span 2
        //
        // might be rewritten into:
        //
        //   <root>
        //    |- span 1
        //    |   |- span 2 (x 3)
        //    |   |- span 3 (x 2)
        //    |- span 2

        struct Context<'a, T> {
            tree_spans: &'a mut Vec<TreeSpan<T>>,
            opts: &'a AsciiOptions,
        }

        /// Check children of tree_spans[id] recursively.
        fn visit<T, K: Eq + Hash>(
            ctx: &mut Context<T>,
            id: usize,
            extract_key: &impl Fn(&TreeSpan<T>) -> Option<K>,
        ) {
            let mut key_to_id = HashMap::<K, usize>::new();
            let child_ids: Vec<usize> = ctx.tree_spans[id].children.to_vec();

            for child_id in child_ids {
                // Do not try to merge this child span if itself, or any of the
                // grand children is interesting. But some of the grand children
                // might be merged. So go visit them.
                if ctx.tree_spans[child_id].is_interesting(ctx.opts, Some(&ctx.tree_spans[id])) || {
                    ctx.tree_spans[child_id].children.iter().any(|&id| {
                        ctx.tree_spans[id].is_interesting(ctx.opts, Some(&ctx.tree_spans[child_id]))
                    })
                } {
                    visit(ctx, child_id, extract_key);
                    continue;
                }

                // Otherwise, attempt to merge from `child_id` to `existing_child_id`.
                if let Some(key) = (extract_key)(&ctx.tree_spans[child_id]) {
                    let existing_child_id = *key_to_id.entry(key).or_insert(child_id);
                    if existing_child_id != child_id {
                        let duration = ctx.tree_spans[child_id].duration;
                        assert_eq!(ctx.tree_spans[child_id].call_count, 1);
                        ctx.tree_spans[child_id].call_count -= 1;
                        let merged = &mut ctx.tree_spans[existing_child_id];
                        merged.call_count += 1;
                        merged.duration += duration;
                    }
                }
            }
        }

        let mut context = Context {
            opts,
            tree_spans: &mut self.0,
        };

        visit(&mut context, 0, extract_key);
    }

    /// Render into ASCII `Rows`.
    pub fn render_ascii_rows(&self, opts: &AsciiOptions, desc: &dyn DescribeTreeSpan<T>) -> Rows {
        struct Context<'a, T> {
            opts: &'a AsciiOptions,
            tree_spans: &'a [TreeSpan<T>],
            desc: &'a dyn DescribeTreeSpan<T>,
        }
        struct Output {
            rows: Vec<Row>,
        }

        /// Render TreeSpans to rows.
        fn render_tree_span<T>(
            ctx: &Context<T>,
            out: &mut Output,
            id: usize,
            mut indent: usize,
            first_row_ch: char,
        ) {
            let span = &ctx.tree_spans[id];
            if span.is_root() {
                return;
            }

            let name = ctx.desc.name(span);
            let source_location = ctx.desc.source(span);
            if !name.is_empty() {
                let start = ctx.desc.start(span);
                let duration = ctx.desc.duration(span);
                let call_count = ctx.desc.call_count(span);

                let first_row = Row {
                    columns: vec![
                        start.to_string(),
                        duration,
                        format!(
                            "{}{} {}{}",
                            " ".repeat(indent),
                            first_row_ch,
                            name,
                            call_count
                        ),
                        source_location,
                    ],
                };
                out.rows.push(first_row);

                // Extra metadata (other than name, source_location)
                let extra_lines = ctx.desc.extra_metadata_lines(span);
                if first_row_ch == '\\' {
                    indent += 1;
                }
                for line in extra_lines.iter() {
                    let row = Row {
                        columns: vec![
                            String::new(),
                            String::new(),
                            format!("{}| {}", " ".repeat(indent), line),
                            format!(":"),
                        ],
                    };
                    out.rows.push(row);
                }
            }
        }

        /// Visit a span and its children recursively.
        fn visit<T>(ctx: &Context<T>, out: &mut Output, id: usize, indent: usize, ch: char) {
            // Print out this span.
            render_tree_span(ctx, out, id, indent, ch);

            // Figure out children to visit.
            let child_ids: Vec<usize> = ctx.tree_spans[id]
                .children
                .iter()
                .cloned()
                .filter(|&id| {
                    ctx.tree_spans[id].is_interesting(ctx.opts, Some(&ctx.tree_spans[id]))
                })
                .collect();

            // Preserve a straight line if there is only one child:
            //
            //   | foo ('bar' is the only child)
            //   | bar  <- case 1
            //
            // Increase indent if there are multi-children (case 2),
            // or it's already not a straight line (case 3):
            //
            //   | foo ('bar1' and 'bar2' are children)
            //    \ bar1     <- case 2
            //     | bar1.1  <- case 3
            //     | bar1.2  <- case 1
            //    \ bar2     <- case 2
            //     \ bar2.1  <- case 2
            //     \ bar2.2  <- case 2
            //
            let (indent, ch) = if child_ids.len() >= 2 {
                // case 2
                (indent + 1, '\\')
            } else if ch == '\\' {
                // case 3
                (indent + 1, '|')
            } else {
                // case 1
                (indent, '|')
            };

            for id in child_ids {
                visit(ctx, out, id, indent, ch)
            }
        }

        let columns = vec![
            "Start".to_string(),
            desc.duration_title(),
            "| Name".to_string(),
            "Source".to_string(),
        ];
        let column_alignments = vec![
            Alignment::Right, // start time
            Alignment::Right, // duration
            Alignment::Left,  // graph, name
            Alignment::Left,  // module, line number
        ];
        let column_min_widths = vec![4, 4, 20, 0];
        let column_max_widths = vec![20, 20, 80, 80];

        let context = Context {
            opts,
            tree_spans: &self.0,
            desc,
        };
        let mut out = Output {
            rows: vec![Row { columns }],
        };

        visit(&context, &mut out, 0, 0, '|');

        Rows {
            rows: out.rows,
            column_alignments,
            column_min_widths,
            column_max_widths,
        }
    }
}
