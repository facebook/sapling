/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;

use super::output::OutputRendererOptions;
use super::output::OutputRendererState;
use super::render::Ancestor;
use super::render::GraphRow;
use super::render::LinkLine;
use super::render::NodeLine;
use super::render::PadLine;
use super::render::Renderer;
use crate::pad::pad_lines;

pub struct AsciiRenderer<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    inner: R,
    state: OutputRendererState,
    _phantom: PhantomData<N>,
}

impl<N, R> AsciiRenderer<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    pub(crate) fn new(inner: R) -> Self {
        AsciiRenderer {
            inner,
            state: OutputRendererState::default(),
            _phantom: PhantomData,
        }
    }

    fn options(&self) -> &OutputRendererOptions {
        self.inner.output_options()
    }
}

impl<N, R> Renderer<N> for AsciiRenderer<N, R>
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
        let line = self.inner.next_row(node, parents, glyph, message);
        let mut out = String::new();
        let mut message_lines = pad_lines(line.message.lines(), self.options().min_row_height);
        let has_term_row = line.term_line.is_some();
        self.state.begin_row(&mut out, line.separator_line);

        // Render the nodeline
        let mut node_line = String::new();
        for entry in line.node_line.iter() {
            match entry {
                NodeLine::Node => {
                    node_line.push_str(&line.glyph);
                    node_line.push(' ');
                }
                NodeLine::Parent => node_line.push_str("| "),
                NodeLine::Ancestor => node_line.push_str(". "),
                NodeLine::Blank => node_line.push_str("  "),
            }
        }
        self.state
            .push_line_with_message(&mut out, node_line, message_lines.next());

        // Render the link line
        if let Some(link_row) = line.link_line {
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
            self.state
                .push_line_with_message(&mut out, link_line, message_lines.next());
        }

        // Render the term line
        if let Some(term_row) = line.term_line {
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
                self.state
                    .push_line_with_message(&mut out, term_line, message_lines.next());
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

        // Render any pad lines
        if !self
            .state
            .push_pad_lines(&mut out, &base_pad_line, message_lines)
            && has_term_row
        {
            self.state.queue_pad_line(base_pad_line);
        }

        out
    }

    fn output_options_mut(&mut self) -> &mut OutputRendererOptions {
        self.inner.output_options_mut()
    }

    fn output_options(&self) -> &OutputRendererOptions {
        self.inner.output_options()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures;
    use super::super::test_fixtures::TestFixture;
    use super::super::test_utils::render_string;
    use crate::GraphRowRenderer;

    fn render(fixture: &TestFixture) -> String {
        let mut renderer = GraphRowRenderer::new().output().build_ascii();
        render_string(fixture, &mut renderer)
    }

    #[test]
    fn basic() {
        assert_eq!(
            render(&test_fixtures::BASIC),
            r#"
            o  C
            |
            o  B
            |
            o  A"#
        );
    }

    #[test]
    fn basic_disconnected() {
        assert_eq!(
            render(&test_fixtures::BASIC_DISCONNECTED),
            r#"
            o  D
            |
            o  C
            
            o  B
            
            o  A"#
        );
    }

    #[test]
    fn basic_disconnected_min_row_height_1() {
        let mut renderer = GraphRowRenderer::new()
            .output()
            .with_min_row_height(1)
            .build_ascii();
        assert_eq!(
            render_string(&test_fixtures::BASIC_DISCONNECTED, &mut renderer),
            r#"
            o  D
            o  C
            
            o  B
            
            o  A"#
        );
    }

    #[test]
    fn basic_disconnected_min_row_height_1_staggered() {
        let mut renderer = GraphRowRenderer::new()
            .output()
            .with_min_row_height(1)
            .with_stagger_consecutive_disconnected_nodes(true)
            .build_ascii();
        assert_eq!(
            render_string(&test_fixtures::BASIC_DISCONNECTED, &mut renderer),
            r#"
            o  D
            o  C
              o  B
            o  A"#
        );
    }

    #[test]
    fn branches_and_merges() {
        assert_eq!(
            render(&test_fixtures::BRANCHES_AND_MERGES),
            r#"
            o  W
            |
            o    V
            |\
            | o    U
            | |\
            | | o  T
            | | |
            | o |  S
            |   |
            o   |  R
            |   |
            o   |  Q
            |\  |
            | o |    P
            | +---.
            | | | o  O
            | | | |
            | | | o    N
            | | | |\
            | o | | |  M
            | | | | |
            | o | | |  L
            | | | | |
            o | | | |  K
            +-------'
            o | | |  J
            | | | |
            o | | |  I
            |/  | |
            o   | |  H
            |   | |
            o   | |  G
            +-----+
            |   | o  F
            |   |/
            |   o  E
            |   |
            o   |  D
            |   |
            o   |  C
            +---'
            o  B
            |
            o  A"#
        );
    }

    #[test]
    fn octopus_branch_and_merge() {
        assert_eq!(
            render(&test_fixtures::OCTOPUS_BRANCH_AND_MERGE),
            r#"
            o      J
            +-+-.
            | | o  I
            | | |
            | o |      H
            +-+-+-+-.
            | | | | o  G
            | | | | |
            | | | o |  E
            | | | |/
            | | o |  D
            | | |\|
            | o | |  C
            | +---'
            o | |  F
            |/  |
            o   |  B
            +---'
            o  A"#
        );
    }

    #[test]
    fn reserved_column() {
        assert_eq!(
            render(&test_fixtures::RESERVED_COLUMN),
            r#"
              o  Z
              |
              o  Y
              |
              o  X
             /
            | o  W
            |/
            o  G
            |
            o    F
            |\
            | o  E
            | |
            | o  D
            |
            o  C
            |
            o  B
            |
            o  A"#
        );
    }

    #[test]
    fn ancestors() {
        assert_eq!(
            render(&test_fixtures::ANCESTORS),
            r#"
              o  Z
              |
              o  Y
             /
            o  F
            .
            . o  X
            ./
            | o  W
            |/
            o  E
            .
            o    D
            |\
            | o  C
            | .
            o .  B
            |/
            o  A"#
        );
    }

    #[test]
    fn split_parents() {
        assert_eq!(
            render(&test_fixtures::SPLIT_PARENTS),
            r#"
                  o  E
            .-+-+-+
            . o | .  D
            ./ \| .
            |   o .  C
            |   |/
            o   |  B
            +---'
            o  A"#
        );
    }

    #[test]
    fn terminations() {
        assert_eq!(
            render(&test_fixtures::TERMINATIONS),
            r#"
              o  K
              |
              | o  J
              |/
              o    I
             /|\
            | | |
            | ~ |
            |   |
            |   o  H
            |   |
            o   |  E
            +---'
            o  D
            |
            ~
            
            o  C
            |
            o  B
            |
            ~"#
        );
    }

    #[test]
    fn long_messages() {
        assert_eq!(
            render(&test_fixtures::LONG_MESSAGES),
            r#"
            o      F
            +-+-.  very long message 1
            | | |  very long message 2
            | | ~  very long message 3
            | |
            | |    very long message 4
            | |    very long message 5
            | |    very long message 6
            | |
            | o  E
            | |
            | o  D
            | |
            o |  C
            |/   long message 1
            |    long message 2
            |    long message 3
            |
            o  B
            |
            o  A
            |  long message 1
            ~  long message 2
               long message 3"#
        );
    }
}
