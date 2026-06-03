/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;

use crate::pipeline::types::GraphRowShape;
use crate::pipeline::types::LinkLine;
use crate::pipeline::types::NodeLine;
use crate::pipeline::types::PadLine;
use crate::pipeline::types::PrefixLine;
use crate::pipeline::types::PrefixLineKind;
use crate::pipeline::types::PrefixLinePart;
use crate::pipeline::types::PrefixLineRenderer;

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

/// Glyph table used by box-drawing prefix line renderers.
pub trait BoxDrawingGlyphSet {
    /// The glyph table for this box-drawing style.
    const GLYPHS: &'static [&'static str; glyph::COUNT];
}

/// Curved box-drawing glyphs.
pub enum Curved {}

/// Square box-drawing glyphs.
pub enum Square {}

/// DEC special graphics glyphs.
pub enum DecGraphics {}

impl BoxDrawingGlyphSet for Curved {
    const GLYPHS: &'static [&'static str; glyph::COUNT] = &CURVED_GLYPHS;
}

impl BoxDrawingGlyphSet for Square {
    const GLYPHS: &'static [&'static str; glyph::COUNT] = &SQUARE_GLYPHS;
}

impl BoxDrawingGlyphSet for DecGraphics {
    const GLYPHS: &'static [&'static str; glyph::COUNT] = &DEC_GLYPHS;
}

/// Converts abstract row shapes into box-drawing graph prefix lines.
pub struct BoxDrawingPrefixLineRenderer<G = Curved> {
    _glyphs: PhantomData<G>,
}

impl<G> BoxDrawingPrefixLineRenderer<G>
where
    G: BoxDrawingGlyphSet,
{
    /// Create a renderer that uses the glyph set from `G`.
    pub fn new() -> Self {
        Self {
            _glyphs: PhantomData,
        }
    }

    /// Use square box-drawing glyphs.
    pub fn with_square_glyphs(self) -> BoxDrawingPrefixLineRenderer<Square> {
        BoxDrawingPrefixLineRenderer::new()
    }

    /// Use DEC special graphics glyphs.
    pub fn with_dec_graphics_glyphs(self) -> BoxDrawingPrefixLineRenderer<DecGraphics> {
        BoxDrawingPrefixLineRenderer::new()
    }
}

impl<G: BoxDrawingGlyphSet> PrefixLineRenderer for BoxDrawingPrefixLineRenderer<G> {
    /// Convert the next graph row shape into prefix lines.
    fn next_prefix_lines<N>(&mut self, line: &GraphRowShape<N>) -> Vec<PrefixLine> {
        let glyphs = G::GLYPHS;
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
                NodeLine::Parent => push_text(&mut node_line, glyphs[glyph::PARENT]),
                NodeLine::Ancestor => push_text(&mut node_line, glyphs[glyph::ANCESTOR]),
                NodeLine::Blank => push_text(&mut node_line, glyphs[glyph::SPACE]),
            }
        }
        lines.push(node_line);

        // Render the link line
        #[allow(clippy::if_same_then_else)]
        if let Some(link_row) = line.link_line.as_ref() {
            let mut link_line = String::new();
            for cur in link_row.iter() {
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
                } else if cur.intersects(LinkLine::RIGHT_FORK) {
                    link_line.push_str(glyphs[glyph::FORK_RIGHT]);
                } else if cur.intersects(LinkLine::RIGHT_MERGE) {
                    link_line.push_str(glyphs[glyph::MERGE_RIGHT]);
                } else {
                    link_line.push_str(glyphs[glyph::SPACE]);
                }
            }
            lines.push(PrefixLine {
                parts: vec![PrefixLinePart::Text(link_line)],
                kind: PrefixLineKind::Link,
            });
        }

        // Render the term line
        if let Some(term_row) = line.term_line.as_ref() {
            let term_strs = [glyphs[glyph::PARENT], glyphs[glyph::TERMINATION]];
            for term_str in term_strs.iter() {
                let mut term_line = String::new();
                for (i, term) in term_row.iter().enumerate() {
                    if *term {
                        term_line.push_str(term_str);
                    } else {
                        term_line.push_str(glyphs[pad_line_to_glyph(line.pad_lines[i])]);
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
            base_pad_line.push_str(glyphs[pad_line_to_glyph(*entry)]);
        }
        lines.push(PrefixLine {
            parts: vec![PrefixLinePart::Text(base_pad_line)],
            kind: PrefixLineKind::PostAncestry,
        });

        lines
    }
}

impl<G> Default for BoxDrawingPrefixLineRenderer<G>
where
    G: BoxDrawingGlyphSet,
{
    fn default() -> Self {
        Self::new()
    }
}

fn push_text(line: &mut PrefixLine, text: &str) {
    line.parts.push(PrefixLinePart::Text(text.to_owned()));
}

fn pad_line_to_glyph(line: PadLine) -> usize {
    match line {
        PadLine::Parent => glyph::PARENT,
        PadLine::Ancestor => glyph::ANCESTOR,
        PadLine::Blank => glyph::SPACE,
    }
}
