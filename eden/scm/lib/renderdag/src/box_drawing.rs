/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;

use itertools::Itertools;

use crate::output::OutputRendererOptions;
use crate::render::{Ancestor, GraphRow, LinkLine, NodeLine, PadLine, Renderer};

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
    glyphs: &'static [&'static str; glyph::COUNT],
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
            glyphs: &CURVED_GLYPHS,
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
        let mut message_lines = line
            .message
            .lines()
            .pad_using(self.options.min_row_height, |_| "");
        let mut need_extra_pad_line = false;

        // Render the previous extra pad line
        if let Some(extra_pad_line) = self.extra_pad_line.take() {
            out.push_str(extra_pad_line.trim_end());
            out.push_str("\n");
        }

        // Render the nodeline
        let mut node_line = String::new();
        for entry in line.node_line.iter() {
            match entry {
                NodeLine::Node => {
                    node_line.push_str(&line.glyph);
                    node_line.push_str(" ");
                }
                NodeLine::Parent => node_line.push_str(glyphs[glyph::PARENT]),
                NodeLine::Ancestor => node_line.push_str(glyphs[glyph::ANCESTOR]),
                NodeLine::Blank => node_line.push_str(glyphs[glyph::SPACE]),
            }
        }
        if let Some(msg) = message_lines.next() {
            node_line.push_str(" ");
            node_line.push_str(msg);
        }
        out.push_str(node_line.trim_end());
        out.push_str("\n");

        // Render the link line
        if let Some(link_row) = line.link_line {
            let mut link_line = String::new();
            for cur in link_row.iter() {
                if cur.contains(LinkLine::HORIZONTAL) {
                    if cur.intersects(LinkLine::CHILD) {
                        link_line.push_str(glyphs[glyph::JOIN_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_FORK)
                        && cur.intersects(LinkLine::ANY_MERGE)
                    {
                        link_line.push_str(glyphs[glyph::JOIN_BOTH]);
                    } else if cur.intersects(LinkLine::ANY_FORK)
                        && cur.intersects(LinkLine::PARENT)
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
                } else if cur.contains(LinkLine::PARENT) && !line.merge {
                    let left = cur.intersects(LinkLine::LEFT_MERGE | LinkLine::LEFT_FORK);
                    let right = cur.intersects(LinkLine::RIGHT_MERGE | LinkLine::RIGHT_FORK);
                    match (left, right) {
                        (true, true) => link_line.push_str(glyphs[glyph::JOIN_BOTH]),
                        (true, false) => link_line.push_str(glyphs[glyph::JOIN_LEFT]),
                        (false, true) => link_line.push_str(glyphs[glyph::JOIN_RIGHT]),
                        (false, false) => link_line.push_str(glyphs[glyph::PARENT]),
                    }
                } else if cur.intersects(LinkLine::PARENT | LinkLine::ANCESTOR)
                    && !cur.intersects(LinkLine::LEFT_FORK | LinkLine::RIGHT_FORK)
                {
                    let left = cur.contains(LinkLine::LEFT_MERGE);
                    let right = cur.contains(LinkLine::RIGHT_MERGE);
                    match (left, right) {
                        (true, true) => link_line.push_str(glyphs[glyph::JOIN_BOTH]),
                        (true, false) => link_line.push_str(glyphs[glyph::JOIN_LEFT]),
                        (false, true) => link_line.push_str(glyphs[glyph::JOIN_RIGHT]),
                        (false, false) => {
                            if cur.contains(LinkLine::ANCESTOR) {
                                link_line.push_str(glyphs[glyph::ANCESTOR]);
                            } else {
                                link_line.push_str(glyphs[glyph::PARENT]);
                            }
                        }
                    }
                } else if cur.contains(LinkLine::LEFT_FORK)
                    && cur.intersects(LinkLine::LEFT_MERGE | LinkLine::CHILD)
                {
                    link_line.push_str(glyphs[glyph::JOIN_LEFT]);
                } else if cur.contains(LinkLine::RIGHT_FORK)
                    && cur.intersects(LinkLine::RIGHT_MERGE | LinkLine::CHILD)
                {
                    link_line.push_str(glyphs[glyph::JOIN_RIGHT]);
                } else if cur.contains(LinkLine::ANY_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_BOTH]);
                } else if cur.contains(LinkLine::ANY_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_BOTH]);
                } else if cur.contains(LinkLine::LEFT_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_LEFT]);
                } else if cur.contains(LinkLine::LEFT_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_LEFT]);
                } else if cur.contains(LinkLine::RIGHT_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_RIGHT]);
                } else if cur.contains(LinkLine::RIGHT_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_RIGHT]);
                } else {
                    link_line.push_str(glyphs[glyph::SPACE]);
                }
            }
            if let Some(msg) = message_lines.next() {
                link_line.push_str(" ");
                link_line.push_str(msg);
            }
            out.push_str(link_line.trim_end());
            out.push_str("\n");
        }

        // Render the term line
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
                    term_line.push_str(" ");
                    term_line.push_str(msg);
                }
                out.push_str(term_line.trim_end());
                out.push_str("\n");
            }
            need_extra_pad_line = true;
        }

        let mut base_pad_line = String::new();
        for entry in line.pad_lines.iter() {
            base_pad_line.push_str(glyphs[entry.to_glyph()]);
        }

        // Render any pad lines
        for msg in message_lines {
            let mut pad_line = base_pad_line.clone();
            pad_line.push_str(" ");
            pad_line.push_str(msg);
            out.push_str(pad_line.trim_end());
            out.push_str("\n");
            need_extra_pad_line = false;
        }

        if need_extra_pad_line {
            self.extra_pad_line = Some(base_pad_line);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use crate::render::GraphRowRenderer;
    use crate::test_fixtures::{self, TestFixture};
    use crate::test_utils::{render_string, render_string_with_order};

    fn render(fixture: &TestFixture) -> String {
        let mut renderer = GraphRowRenderer::new().output().build_box_drawing();
        render_string(fixture, &mut renderer)
    }

    #[test]
    fn basic() {
        assert_eq!(
            render(&test_fixtures::BASIC),
            r#"
            o  C
            │
            o  B
            │
            o  A"#
        );
    }

    #[test]
    fn branches_and_merges() {
        assert_eq!(
            render(&test_fixtures::BRANCHES_AND_MERGES),
            r#"
            o  W
            │
            o    V
            ├─╮
            │ o    U
            │ ├─╮
            │ │ o  T
            │ │ │
            │ o │  S
            │   │
            o   │  R
            │   │
            o   │  Q
            ├─╮ │
            │ o │    P
            │ ├───╮
            │ │ │ o  O
            │ │ │ │
            │ │ │ o    N
            │ │ │ ├─╮
            │ o │ │ │  M
            │ │ │ │ │
            │ o │ │ │  L
            │ │ │ │ │
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
            │   │ o  F
            │   ├─╯
            │   o  E
            │   │
            o   │  D
            │   │
            o   │  C
            ├───╯
            o  B
            │
            o  A"#
        );
    }

    #[test]
    fn octopus_branch_and_merge() {
        assert_eq!(
            render(&test_fixtures::OCTOPUS_BRANCH_AND_MERGE),
            r#"
            o      J
            ├─┬─╮
            │ │ o  I
            │ │ │
            │ o │      H
            ╭─┼─┬─┬─╮
            │ │ │ │ o  G
            │ │ │ │ │
            │ │ │ o │  E
            │ │ │ ├─╯
            │ │ o │  D
            │ │ ├─╮
            │ o │ │  C
            │ ├───╯
            o │ │  F
            ├─╯ │
            o   │  B
            ├───╯
            o  A"#
        );
    }

    #[test]
    fn reserved_column() {
        assert_eq!(
            render(&test_fixtures::RESERVED_COLUMN),
            r#"
              o  Z
              │
              o  Y
              │
              o  X
            ╭─╯
            │ o  W
            ├─╯
            o  G
            │
            o    F
            ├─╮
            │ o  E
            │ │
            │ o  D
            │
            o  C
            │
            o  B
            │
            o  A"#
        );
    }

    #[test]
    fn ancestors() {
        assert_eq!(
            render(&test_fixtures::ANCESTORS),
            r#"
              o  Z
              │
              o  Y
            ╭─╯
            o  F
            ╷
            ╷ o  X
            ╭─╯
            │ o  W
            ├─╯
            o  E
            ╷
            o    D
            ├─╮
            │ o  C
            │ ╷
            o ╷  B
            ├─╯
            o  A"#
        );
    }

    #[test]
    fn split_parents() {
        assert_eq!(
            render(&test_fixtures::SPLIT_PARENTS),
            r#"
                  o  E
            ╭─┬─┬─┤
            ╷ o │ ╷  D
            ╭─┴─╮ ╷
            │   o ╷  C
            │   ├─╯
            o   │  B
            ├───╯
            o  A"#
        );
    }

    #[test]
    fn terminations() {
        assert_eq!(
            render(&test_fixtures::TERMINATIONS),
            r#"
              o  K
              │
              │ o  J
              ├─╯
              o    I
            ╭─┼─╮
            │ │ │
            │ ~ │
            │   │
            │   o  H
            │   │
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
        );
    }

    #[test]
    fn long_messages() {
        assert_eq!(
            render(&test_fixtures::LONG_MESSAGES),
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
            │ o  E
            │ │
            │ o  D
            │ │
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
        );
    }

    #[test]
    fn different_orders() {
        let order = |order: &str| {
            let order = order.matches(|_: char| true).collect::<Vec<_>>();
            let mut renderer = GraphRowRenderer::new().output().build_box_drawing();
            render_string_with_order(&test_fixtures::ORDERS1, &mut renderer, Some(&order))
        };

        assert_eq!(
            order("KJIHGFEDCBZA"),
            r#"
            o    K
            ├─╮
            │ o    J
            │ ├─╮
            │ │ o    I
            │ │ ├─╮
            │ │ │ o    H
            │ │ │ ├─╮
            │ │ │ │ o    G
            │ │ │ │ ├─╮
            o │ │ │ │ │  F
            │ │ │ │ │ │
            │ o │ │ │ │  E
            ├─╯ │ │ │ │
            │   o │ │ │  D
            ├───╯ │ │ │
            │     o │ │  C
            ├─────╯ │ │
            │       o │  B
            ├───────╯ │
            │         o  Z
            │
            o  A"#
        );

        assert_eq!(
            order("KJIHGZBCDEFA"),
            r#"
            o    K
            ├─╮
            │ o    J
            │ ├─╮
            │ │ o    I
            │ │ ├─╮
            │ │ │ o    H
            │ │ │ ├─╮
            │ │ │ │ o    G
            │ │ │ │ ├─╮
            │ │ │ │ │ o  Z
            │ │ │ │ │
            │ │ │ │ o  B
            │ │ │ │ │
            │ │ │ o │  C
            │ │ │ ├─╯
            │ │ o │  D
            │ │ ├─╯
            │ o │  E
            │ ├─╯
            o │  F
            ├─╯
            o  A"#
        );

        // Keeping the p1 branch the longest path (KFEDCBA) is a reasonable
        // optimization for a cleaner graph (less columns, more text space).
        assert_eq!(
            render(&test_fixtures::ORDERS2),
            r#"
            o    K
            ├─╮
            │ o  J
            │ │
            o │    F
            ├───╮
            │ │ o  I
            │ ├─╯
            o │    E
            ├───╮
            │ │ o  H
            │ ├─╯
            o │    D
            ├───╮
            │ │ o  G
            │ ├─╯
            o │    C
            ├───╮
            │ │ o  Z
            │ │
            o │  B
            ├─╯
            o  A"#
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
            order("KFJEIDHCGZBA"),
            r#"
            o    K
            ├─╮
            o │  F
            │ │
            │ o    J
            │ ├─╮
            │ o │  E
            ├─╯ │
            │   o  I
            │ ╭─┤
            │ │ o  D
            ├───╯
            │ o    H
            │ ├─╮
            │ o │  C
            ├─╯ │
            │   o  G
            │ ╭─┤
            │ o │  Z
            │   │
            │   o  B
            ├───╯
            o  A"#
        );
    }
}
