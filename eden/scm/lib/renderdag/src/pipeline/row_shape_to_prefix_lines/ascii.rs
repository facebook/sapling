/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::pipeline::types::GraphRowShape;
use crate::pipeline::types::LinkLine;
use crate::pipeline::types::NodeLine;
use crate::pipeline::types::PadLine;
use crate::pipeline::types::PrefixLine;
use crate::pipeline::types::PrefixLineKind;
use crate::pipeline::types::PrefixLinePart;

/// Converts abstract row shapes into ASCII graph prefix lines.
#[derive(Default)]
pub struct AsciiPrefixLineRenderer;

impl AsciiPrefixLineRenderer {
    /// Create an ASCII prefix line renderer.
    pub fn new() -> Self {
        Self
    }

    /// Convert the next graph row shape into prefix lines.
    pub fn next_prefix_lines<N>(&mut self, line: &GraphRowShape<N>) -> Vec<PrefixLine> {
        let mut lines = Vec::new();

        // Render the nodeline
        let mut node_line = PrefixLine {
            parts: Vec::new(),
            kind: PrefixLineKind::Node,
        };
        for entry in line.node_line.iter() {
            match entry {
                NodeLine::Node => {
                    node_line.parts.push(PrefixLinePart::NodeGlyph);
                    node_line
                        .parts
                        .push(PrefixLinePart::Text(String::from(" ")));
                }
                NodeLine::Parent => push_text(&mut node_line, "| "),
                NodeLine::Ancestor => push_text(&mut node_line, ". "),
                NodeLine::Blank => push_text(&mut node_line, "  "),
            }
        }
        lines.push(node_line);

        // Render the link line
        if let Some(link_row) = line.link_line.as_ref() {
            let mut link_line = String::new();
            let any_horizontal = link_row
                .iter()
                .any(|cur| cur.intersects(LinkLine::HORIZONTAL));
            let mut iter = link_row
                .iter()
                .copied()
                .chain(std::iter::once(LinkLine::empty()))
                .peekable();
            while let Some(cur) = iter.next() {
                let next = match iter.peek() {
                    Some(&v) => v,
                    None => break,
                };
                // Draw the parent/ancestor line.
                if cur.intersects(LinkLine::HORIZONTAL) {
                    if cur.intersects(LinkLine::CHILD | LinkLine::ANY_FORK_OR_MERGE) {
                        link_line.push('+');
                    } else {
                        link_line.push('-');
                    }
                } else if cur.intersects(LinkLine::VERTICAL) {
                    if cur.intersects(LinkLine::ANY_FORK_OR_MERGE) && any_horizontal {
                        link_line.push('+');
                    } else if cur.intersects(LinkLine::VERT_PARENT) {
                        link_line.push('|');
                    } else {
                        link_line.push('.');
                    }
                } else if cur.intersects(LinkLine::ANY_MERGE) && any_horizontal {
                    link_line.push('\'');
                } else if cur.intersects(LinkLine::ANY_FORK) && any_horizontal {
                    link_line.push('.');
                } else {
                    link_line.push(' ');
                }

                // Draw the connecting line.
                if cur.intersects(LinkLine::HORIZONTAL) {
                    link_line.push('-');
                } else if cur.intersects(LinkLine::RIGHT_MERGE) {
                    if next.intersects(LinkLine::LEFT_FORK) && !any_horizontal {
                        link_line.push('\\');
                    } else {
                        link_line.push('-');
                    }
                } else if cur.intersects(LinkLine::RIGHT_FORK) {
                    if next.intersects(LinkLine::LEFT_MERGE) && !any_horizontal {
                        link_line.push('/');
                    } else {
                        link_line.push('-');
                    }
                } else {
                    link_line.push(' ');
                }
            }
            lines.push(PrefixLine {
                parts: vec![PrefixLinePart::Text(link_line)],
                kind: PrefixLineKind::Link,
            });
        }

        // Render the term line
        if let Some(term_row) = line.term_line.as_ref() {
            let term_strs = ["| ", "~ "];
            for term_str in term_strs.iter() {
                let mut term_line = String::new();
                for (i, term) in term_row.iter().enumerate() {
                    if *term {
                        term_line.push_str(term_str);
                    } else {
                        term_line.push_str(match line.pad_lines[i] {
                            PadLine::Parent => "| ",
                            PadLine::Ancestor => ". ",
                            PadLine::Blank => "  ",
                        });
                    }
                }
                lines.push(PrefixLine {
                    parts: vec![PrefixLinePart::Text(term_line)],
                    kind: PrefixLineKind::Term,
                });
            }
        }

        let mut base_pad_line = String::new();
        for entry in line.pad_lines.iter() {
            base_pad_line.push_str(match entry {
                PadLine::Parent => "| ",
                PadLine::Ancestor => ". ",
                PadLine::Blank => "  ",
            });
        }
        lines.push(PrefixLine {
            parts: vec![PrefixLinePart::Text(base_pad_line)],
            kind: PrefixLineKind::PostAncestry,
        });

        lines
    }
}

fn push_text(line: &mut PrefixLine, text: &str) {
    line.parts.push(PrefixLinePart::Text(text.to_owned()));
}
