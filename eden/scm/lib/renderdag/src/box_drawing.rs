/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;

use super::output::OutputRendererOptions;
use super::render::Ancestor;
use super::render::GraphRow;
use super::render::LinkLine;
use super::render::NodeLine;
use super::render::PadLine;
use super::render::Renderer;
use crate::pad::pad_lines;

mod glyph {
    pub(super) const SPACE: usize = 0;
    pub(super) const HORIZONTAL: usize = 1;
    pub(super) const PARENT: usize = 2;
    pub(super) const ANCESTOR: usize = 3;
    pub(super) const MERGE_LEFT: usize = 4;
    pub(super) const MERGE_RIGHT: usize = 5;
    pub(super) const MERGE_BOTH: usize = 6;
    pub(super) const FORK_LEFT: usize = 7;
    pub(super) const FORK_RIGHT: usize = 8;
    pub(super) const FORK_BOTH: usize = 9;
    pub(super) const JOIN_LEFT: usize = 10;
    pub(super) const JOIN_RIGHT: usize = 11;
    pub(super) const JOIN_BOTH: usize = 12;
    pub(super) const TERMINATION: usize = 13;
    pub(super) const COUNT: usize = 14;
}

const SQUARE_GLYPHS: [&str; glyph::COUNT] = [
    "  ", "──", "│ ", "· ", "┘ ", "└─", "┴─", "┐ ", "┌─", "┬─", "┤ ", "├─", "┼─", "~ ",
];

const CURVED_GLYPHS: [&str; glyph::COUNT] = [
    "  ", "──", "│ ", "╷ ", "╯ ", "╰─", "┴─", "╮ ", "╭─", "┬─", "┤ ", "├─", "┼─", "~ ",
];

const DEC_GLYPHS: [&str; glyph::COUNT] = [
    "  ",
    "\x1B(0qq\x1B(B",
    "\x1B(0x \x1B(B",
    "\x1B(0~ \x1B(B",
    "\x1B(0j \x1B(B",
    "\x1B(0mq\x1B(B",
    "\x1B(0vq\x1B(B",
    "\x1B(0k \x1B(B",
    "\x1B(0lq\x1B(B",
    "\x1B(0wq\x1B(B",
    "\x1B(0u \x1B(B",
    "\x1B(0tq\x1B(B",
    "\x1B(0nq\x1B(B",
    "~ ",
];

impl PadLine {
    fn to_glyph(&self) -> usize {
        match *self {
            PadLine::Parent => glyph::PARENT,
            PadLine::Ancestor => glyph::ANCESTOR,
            PadLine::Blank => glyph::SPACE,
        }
    }
}

pub struct BoxDrawingRenderer<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    inner: R,
    options: OutputRendererOptions,
    extra_pad_line: Option<String>,
    insert_extra_pad_line: bool,
    last_node_column: Option<usize>,
    glyphs: &'static [&'static str; glyph::COUNT],
    pad_between_branches: bool,
    _phantom: PhantomData<N>,
}

impl<N, R> BoxDrawingRenderer<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    pub(crate) fn new(inner: R, options: OutputRendererOptions) -> Self {
        BoxDrawingRenderer {
            inner,
            options,
            extra_pad_line: None,
            insert_extra_pad_line: false,
            last_node_column: None,
            glyphs: &CURVED_GLYPHS,
            pad_between_branches: false,
            _phantom: PhantomData,
        }
    }

    pub fn with_square_glyphs(mut self) -> Self {
        self.glyphs = &SQUARE_GLYPHS;
        self
    }

    pub fn with_dec_graphics_glyphs(mut self) -> Self {
        self.glyphs = &DEC_GLYPHS;
        self
    }

    pub fn with_branch_padding(mut self) -> Self {
        self.pad_between_branches = true;
        self
    }
}

impl<N, R> Renderer<N> for BoxDrawingRenderer<N, R>
where
    N: Clone + Eq,
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    type Output = String;

    fn width(&self, node: Option<&N>, parents: Option<&Vec<Ancestor<N>>>) -> u64 {
        self.inner
            .width(node, parents)
            .saturating_mul(2)
            .saturating_add(1)
    }

    fn reserve(&mut self, node: N) {
        self.inner.reserve(node);
    }

    fn next_row(
        &mut self,
        node: N,
        parents: Vec<Ancestor<N>>,
        glyph: String,
        message: String,
    ) -> String {
        let glyphs = self.glyphs;
        let line = self.inner.next_row(node, parents, glyph, message);
        let mut out = String::new();
        let mut message_lines = pad_lines(line.message.lines(), self.options.min_row_height);
        let mut need_extra_pad_line = false;

        // Construct the nodeline, which has the graph node symbol
        let mut node_line = String::new();
        let mut node_column = None;
        for (i, entry) in line.node_line.iter().enumerate() {
            match entry {
                NodeLine::Node => {
                    node_line.push_str(&line.glyph);
                    node_line.push(' ');
                    node_column = Some(i);
                }
                NodeLine::Parent => node_line.push_str(glyphs[glyph::PARENT]),
                NodeLine::Ancestor => node_line.push_str(glyphs[glyph::ANCESTOR]),
                NodeLine::Blank => node_line.push_str(glyphs[glyph::SPACE]),
            }
        }
        if let Some(msg) = message_lines.next() {
            node_line.push(' ');
            node_line.push_str(msg);
        }

        // The last node was in a different column, so force an extra pad line
        // to separate the nodes visually
        if self.pad_between_branches && node_column != self.last_node_column {
            self.insert_extra_pad_line = true;
        }

        // Render the previous extra pad line
        if self.insert_extra_pad_line
            && let Some(extra_pad_line) = self.extra_pad_line.take()
        {
            out.push_str(extra_pad_line.clone().trim_end());
            out.push('\n');
        }

        // Render the nodeline
        out.push_str(node_line.trim_end());
        out.push('\n');

        let mut next_node_is_other_branch = false;

        // Render the link line, to show branches and merges
        #[allow(clippy::if_same_then_else)]
        if let Some(link_row) = line.link_line {
            let mut link_line = String::new();
            for (col, cur) in link_row.iter().enumerate() {
                if cur.intersects(LinkLine::HORIZONTAL) {
                    if cur.intersects(LinkLine::CHILD) {
                        link_line.push_str(glyphs[glyph::JOIN_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_FORK)
                        && cur.intersects(LinkLine::ANY_MERGE)
                    {
                        link_line.push_str(glyphs[glyph::JOIN_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_FORK)
                        && cur.intersects(LinkLine::VERT_PARENT)
                        && !line.merge
                    {
                        link_line.push_str(glyphs[glyph::JOIN_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_FORK) {
                        link_line.push_str(glyphs[glyph::FORK_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_MERGE) {
                        link_line.push_str(glyphs[glyph::MERGE_BOTH]);
                    } else {
                        link_line.push_str(glyphs[glyph::HORIZONTAL]);
                    }
                } else if cur.intersects(LinkLine::VERT_PARENT) && !line.merge {
                    let left = cur.intersects(LinkLine::LEFT_MERGE | LinkLine::LEFT_FORK);
                    let right = cur.intersects(LinkLine::RIGHT_MERGE | LinkLine::RIGHT_FORK);
                    match (left, right) {
                        (true, true) => link_line.push_str(glyphs[glyph::JOIN_BOTH]),
                        (true, false) => link_line.push_str(glyphs[glyph::JOIN_LEFT]),
                        (false, true) => link_line.push_str(glyphs[glyph::JOIN_RIGHT]),
                        (false, false) => link_line.push_str(glyphs[glyph::PARENT]),
                    }
                } else if cur.intersects(LinkLine::VERT_PARENT | LinkLine::VERT_ANCESTOR)
                    && !cur.intersects(LinkLine::LEFT_FORK | LinkLine::RIGHT_FORK)
                {
                    let left = cur.intersects(LinkLine::LEFT_MERGE);
                    let right = cur.intersects(LinkLine::RIGHT_MERGE);
                    match (left, right) {
                        (true, true) => link_line.push_str(glyphs[glyph::JOIN_BOTH]),
                        (true, false) => link_line.push_str(glyphs[glyph::JOIN_LEFT]),
                        (false, true) => link_line.push_str(glyphs[glyph::JOIN_RIGHT]),
                        (false, false) => {
                            if cur.intersects(LinkLine::VERT_ANCESTOR) {
                                link_line.push_str(glyphs[glyph::ANCESTOR]);
                            } else {
                                link_line.push_str(glyphs[glyph::PARENT]);
                            }
                        }
                    }
                } else if cur.intersects(LinkLine::LEFT_FORK)
                    && cur.intersects(LinkLine::LEFT_MERGE | LinkLine::CHILD)
                {
                    link_line.push_str(glyphs[glyph::JOIN_LEFT]);
                } else if cur.intersects(LinkLine::RIGHT_FORK)
                    && cur.intersects(LinkLine::RIGHT_MERGE | LinkLine::CHILD)
                {
                    link_line.push_str(glyphs[glyph::JOIN_RIGHT]);
                } else if cur.intersects(LinkLine::LEFT_MERGE)
                    && cur.intersects(LinkLine::RIGHT_MERGE)
                {
                    link_line.push_str(glyphs[glyph::MERGE_BOTH]);
                } else if cur.intersects(LinkLine::LEFT_FORK)
                    && cur.intersects(LinkLine::RIGHT_FORK)
                {
                    link_line.push_str(glyphs[glyph::FORK_BOTH]);
                } else if cur.intersects(LinkLine::LEFT_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_LEFT]);
                } else if cur.intersects(LinkLine::LEFT_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_LEFT]);
                    // This is the only case where two consecutive nodes that
                    // share the same column belong to different branches
                    if self.pad_between_branches && node_column.is_some_and(|nc| nc == col) {
                        next_node_is_other_branch = true;
                    }
                } else if cur.intersects(LinkLine::RIGHT_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_RIGHT]);
                } else if cur.intersects(LinkLine::RIGHT_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_RIGHT]);
                } else {
                    link_line.push_str(glyphs[glyph::SPACE]);
                }
            }
            if let Some(msg) = message_lines.next() {
                link_line.push(' ');
                link_line.push_str(msg);
            }
            out.push_str(link_line.trim_end());
            out.push('\n');
        }

        // Render any term line, to indicate terminated branches
        if let Some(term_row) = line.term_line {
            let term_strs = [glyphs[glyph::PARENT], glyphs[glyph::TERMINATION]];
            for term_str in term_strs.iter() {
                let mut term_line = String::new();
                for (i, term) in term_row.iter().enumerate() {
                    if *term {
                        term_line.push_str(term_str);
                    } else {
                        term_line.push_str(glyphs[line.pad_lines[i].to_glyph()]);
                    }
                }
                if let Some(msg) = message_lines.next() {
                    term_line.push(' ');
                    term_line.push_str(msg);
                }
                out.push_str(term_line.trim_end());
                out.push('\n');
            }
            need_extra_pad_line = true;
        }

        let mut base_pad_line = String::new();
        for entry in line.pad_lines.iter() {
            base_pad_line.push_str(glyphs[entry.to_glyph()]);
        }

        // Render any pad lines, to fit multi-line messages
        for msg in message_lines {
            let mut pad_line = base_pad_line.clone();
            pad_line.push(' ');
            pad_line.push_str(msg);
            out.push_str(pad_line.trim_end());
            out.push('\n');
            need_extra_pad_line = false;
        }

        self.extra_pad_line = Some(base_pad_line);
        self.insert_extra_pad_line = need_extra_pad_line || next_node_is_other_branch;
        self.last_node_column = node_column;

        out
    }
}

#[cfg(test)]
mod tests {
    use core::fmt;

    use super::super::test_fixtures;
    use super::super::test_fixtures::TestFixture;
    use super::super::test_utils::render_string;
    use super::super::test_utils::render_string_with_order;
    use crate::GraphRowRenderer;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum BranchPadding {
        Yes,
        No,
    }

    impl std::fmt::Display for BranchPadding {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            fmt::Debug::fmt(self, f)
        }
    }

    /// Type for rendering the graph strings with newlines in asserts.
    #[derive(PartialEq, Eq)]
    struct GraphString(String);

    impl std::fmt::Debug for GraphString {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0.replace("\\n", "\n"))
        }
    }

    impl From<&str> for GraphString {
        fn from(value: &str) -> Self {
            GraphString(value.to_owned())
        }
    }

    fn render(fixture: &TestFixture, with_branch_pad: BranchPadding) -> GraphString {
        let mut renderer = GraphRowRenderer::new().output().build_box_drawing();
        if with_branch_pad == BranchPadding::Yes {
            renderer = renderer.with_branch_padding();
        }
        GraphString(render_string(fixture, &mut renderer))
    }

    /// Filters the expected graph string according to the options
    ///
    /// If a line ends with ` <branch pad>`, it will only be included if
    /// `with_branch_pad` is `Yes`.
    fn filter(with_branch_pad: BranchPadding, expected: &str) -> GraphString {
        let mut filtered = String::new();
        for mut line in expected.lines() {
            if let Some(pad_pos) = line.find(" <branch pad>") {
                match with_branch_pad {
                    BranchPadding::Yes => line = line.split_at(pad_pos).0,
                    BranchPadding::No => continue,
                }
            }
            filtered.push_str(line);
            filtered.push('\n');
        }
        if !filtered.is_empty() {
            filtered.pop();
        }
        GraphString(filtered)
    }

    #[test]
    fn basic() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::BASIC, branch_pad),
                filter(
                    branch_pad,
                    r#"
            o  C
            │
            o  B
            │
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn branches_and_merges() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::BRANCHES_AND_MERGES, branch_pad),
                filter(
                    branch_pad,
                    r#"
            o  W
            │
            o    V
            ├─╮
            │ │ <branch pad>
            │ o    U
            │ ├─╮
            │ │ │ <branch pad>
            │ │ o  T
            │ │ │
            │ │ │ <branch pad>
            │ o │  S
            │   │
            │   │ <branch pad>
            o   │  R
            │   │
            o   │  Q
            ├─╮ │
            │ │ │ <branch pad>
            │ o │    P
            │ ├───╮
            │ │ │ │ <branch pad>
            │ │ │ o  O
            │ │ │ │
            │ │ │ o    N
            │ │ │ ├─╮
            │ │ │ │ │ <branch pad>
            │ o │ │ │  M
            │ │ │ │ │
            │ o │ │ │  L
            │ │ │ │ │
            │ │ │ │ │ <branch pad>
            o │ │ │ │  K
            ├───────╯
            o │ │ │  J
            │ │ │ │
            o │ │ │  I
            ├─╯ │ │
            o   │ │  H
            │   │ │
            o   │ │  G
            ├─────╮
            │   │ │ <branch pad>
            │   │ o  F
            │   ├─╯
            │   │ <branch pad>
            │   o  E
            │   │
            │   │ <branch pad>
            o   │  D
            │   │
            o   │  C
            ├───╯
            o  B
            │
            o  A"#
                ),
                "branch_pad: {}",
                branch_pad
            );
        }
    }

    #[test]
    fn octopus_branch_and_merge() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::OCTOPUS_BRANCH_AND_MERGE, branch_pad),
                filter(
                    branch_pad,
                    r#"
            o      J
            ├─┬─╮
            │ │ │ <branch pad>
            │ │ o  I
            │ │ │
            │ │ │ <branch pad>
            │ o │      H
            ╭─┼─┬─┬─╮
            │ │ │ │ │ <branch pad>
            │ │ │ │ o  G
            │ │ │ │ │
            │ │ │ │ │ <branch pad>
            │ │ │ o │  E
            │ │ │ ├─╯
            │ │ │ │ <branch pad>
            │ │ o │  D
            │ │ ├─╮
            │ │ │ │ <branch pad>
            │ o │ │  C
            │ ├───╯
            │ │ │ <branch pad>
            o │ │  F
            ├─╯ │
            o   │  B
            ├───╯
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn reserved_column() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::RESERVED_COLUMN, branch_pad),
                filter(
                    branch_pad,
                    r#"
              o  Z
              │
              o  Y
              │
              o  X
            ╭─╯
            │ <branch pad>
            │ o  W
            ├─╯
            │ <branch pad>
            o  G
            │
            o    F
            ├─╮
            │ │ <branch pad>
            │ o  E
            │ │
            │ o  D
            │
            │ <branch pad>
            o  C
            │
            o  B
            │
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn ancestors() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::ANCESTORS, branch_pad),
                filter(
                    branch_pad,
                    r#"
              o  Z
              │
              o  Y
            ╭─╯
            │ <branch pad>
            o  F
            ╷
            ╷ <branch pad>
            ╷ o  X
            ╭─╯
            │ <branch pad>
            │ o  W
            ├─╯
            │ <branch pad>
            o  E
            ╷
            o    D
            ├─╮
            │ ╷ <branch pad>
            │ o  C
            │ ╷
            │ ╷ <branch pad>
            o ╷  B
            ├─╯
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn split_parents() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::SPLIT_PARENTS, branch_pad),
                filter(
                    branch_pad,
                    r#"
                  o  E
            ╭─┬─┬─┤
            ╷ │ │ ╷ <branch pad>
            ╷ o │ ╷  D
            ╭─┴─╮ ╷
            │   │ ╷ <branch pad>
            │   o ╷  C
            │   ├─╯
            │   │ <branch pad>
            o   │  B
            ├───╯
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn terminations() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::TERMINATIONS, branch_pad),
                filter(
                    branch_pad,
                    r#"
              o  K
              │
              │ <branch pad>
              │ o  J
              ├─╯
              │ <branch pad>
              o    I
            ╭─┼─╮
            │ │ │
            │ ~ │
            │   │
            │   o  H
            │   │
            │   │ <branch pad>
            o   │  E
            ├───╯
            o  D
            │
            ~
            
            o  C
            │
            o  B
            │
            ~"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn long_messages() {
        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                render(&test_fixtures::LONG_MESSAGES, branch_pad),
                filter(
                    branch_pad,
                    r#"
            o      F
            ├─┬─╮  very long message 1
            │ │ │  very long message 2
            │ │ ~  very long message 3
            │ │
            │ │    very long message 4
            │ │    very long message 5
            │ │    very long message 6
            │ │
            │ │ <branch pad>
            │ o  E
            │ │
            │ o  D
            │ │
            │ │ <branch pad>
            o │  C
            ├─╯  long message 1
            │    long message 2
            │    long message 3
            │
            o  B
            │
            o  A
            │  long message 1
            ~  long message 2
               long message 3"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }

    #[test]
    fn different_orders() {
        let order = |order: &str, branch_pad: BranchPadding| {
            let order = order.matches(|_: char| true).collect::<Vec<_>>();
            let mut renderer = GraphRowRenderer::new().output().build_box_drawing();
            if branch_pad == BranchPadding::Yes {
                renderer = renderer.with_branch_padding();
            }
            GraphString(render_string_with_order(
                &test_fixtures::ORDERS1,
                &mut renderer,
                Some(&order),
            ))
        };

        for branch_pad in [BranchPadding::No, BranchPadding::Yes] {
            assert_eq!(
                order("KJIHGFEDCBZA", branch_pad),
                filter(
                    branch_pad,
                    r#"
            o    K
            ├─╮
            │ │ <branch pad>
            │ o    J
            │ ├─╮
            │ │ │ <branch pad>
            │ │ o    I
            │ │ ├─╮
            │ │ │ │ <branch pad>
            │ │ │ o    H
            │ │ │ ├─╮
            │ │ │ │ │ <branch pad>
            │ │ │ │ o    G
            │ │ │ │ ├─╮
            │ │ │ │ │ │ <branch pad>
            o │ │ │ │ │  F
            │ │ │ │ │ │
            │ │ │ │ │ │ <branch pad>
            │ o │ │ │ │  E
            ├─╯ │ │ │ │
            │   │ │ │ │ <branch pad>
            │   o │ │ │  D
            ├───╯ │ │ │
            │     │ │ │ <branch pad>
            │     o │ │  C
            ├─────╯ │ │
            │       │ │ <branch pad>
            │       o │  B
            ├───────╯ │
            │         │ <branch pad>
            │         o  Z
            │
            │ <branch pad>
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );

            assert_eq!(
                order("KJIHGZBCDEFA", branch_pad),
                filter(
                    branch_pad,
                    r#"
            o    K
            ├─╮
            │ │ <branch pad>
            │ o    J
            │ ├─╮
            │ │ │ <branch pad>
            │ │ o    I
            │ │ ├─╮
            │ │ │ │ <branch pad>
            │ │ │ o    H
            │ │ │ ├─╮
            │ │ │ │ │ <branch pad>
            │ │ │ │ o    G
            │ │ │ │ ├─╮
            │ │ │ │ │ │ <branch pad>
            │ │ │ │ │ o  Z
            │ │ │ │ │
            │ │ │ │ │ <branch pad>
            │ │ │ │ o  B
            │ │ │ │ │
            │ │ │ │ │ <branch pad>
            │ │ │ o │  C
            │ │ │ ├─╯
            │ │ │ │ <branch pad>
            │ │ o │  D
            │ │ ├─╯
            │ │ │ <branch pad>
            │ o │  E
            │ ├─╯
            │ │ <branch pad>
            o │  F
            ├─╯
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );

            // Keeping the p1 branch the longest path (KFEDCBA) is a reasonable
            // optimization for a cleaner graph (less columns, more text space).
            assert_eq!(
                render(&test_fixtures::ORDERS2, branch_pad),
                filter(
                    branch_pad,
                    r#"
            o    K
            ├─╮
            │ │ <branch pad>
            │ o  J
            │ │
            │ │ <branch pad>
            o │    F
            ├───╮
            │ │ │ <branch pad>
            │ │ o  I
            │ ├─╯
            │ │ <branch pad>
            o │    E
            ├───╮
            │ │ │ <branch pad>
            │ │ o  H
            │ ├─╯
            │ │ <branch pad>
            o │    D
            ├───╮
            │ │ │ <branch pad>
            │ │ o  G
            │ ├─╯
            │ │ <branch pad>
            o │    C
            ├───╮
            │ │ │ <branch pad>
            │ │ o  Z
            │ │
            │ │ <branch pad>
            o │  B
            ├─╯
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );

            // Try to use the ORDERS2 order. However, the parent ordering in the
            // graph is different, which makes the rendering different.
            //
            // Note: it's KJFIEHDGCZBA in the ORDERS2 graph. To map it to ORDERS1,
            // follow:
            //
            // ORDERS1: KFJEIDHCGBZA
            // ORDERS2: KJFIEHDGCBZA
            //
            // And we get KFJEIDHCGZBA.
            assert_eq!(
                order("KFJEIDHCGZBA", branch_pad),
                filter(
                    branch_pad,
                    r#"
            o    K
            ├─╮
            o │  F
            │ │
            │ │ <branch pad>
            │ o    J
            │ ├─╮
            │ o │  E
            ├─╯ │
            │   │ <branch pad>
            │   o  I
            │ ╭─┤
            │ │ o  D
            ├───╯
            │ │ <branch pad>
            │ o    H
            │ ├─╮
            │ o │  C
            ├─╯ │
            │   │ <branch pad>
            │   o  G
            │ ╭─┤
            │ │ │ <branch pad>
            │ o │  Z
            │   │
            │   │ <branch pad>
            │   o  B
            ├───╯
            │ <branch pad>
            o  A"#
                ),
                "branch_pad: {branch_pad}"
            );
        }
    }
}
