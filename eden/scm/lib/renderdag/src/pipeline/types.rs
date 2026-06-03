/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(feature = "serialize")]
use serde::Serialize;

pub use crate::render::Ancestor;
pub use crate::render::LinkLine;
pub use crate::render::NodeLine;
pub use crate::render::PadLine;

/// Options that affect the graph row shape produced from the input node stream.
///
/// These options belong to the first pipeline stage. They may affect column
/// allocation, separator rows, and the abstract edge shape. They do not choose
/// glyph characters or place message text.
pub type GraphRowShapeOptions = crate::OutputRendererOptions;

/// An abstract graph row shape for one rendered node.
///
/// This is the output of the graph-to-rows stage. It captures the node's
/// current column, the edge shape around it, and any row-level facts needed by
/// later stages. It intentionally does not contain glyphs or message text.
#[derive(Clone, Debug, PartialEq, Eq)]
// This is `Serialize` so non-text environment (e.g. via wasm) can get
// the rich data (than just ambiguous text characters) to render more
// precisely. Example: `RenderDag.js` in D41252231.
#[cfg_attr(feature = "serialize", derive(Serialize))]
pub struct GraphRowShape<N> {
    /// The node represented by this row.
    pub node: N,

    /// True if this row connects to multiple parents.
    ///
    /// Some prefix renderers use this to choose clearer junction glyphs.
    pub merge: bool,

    /// True if this row needs a blank separator before its graph lines.
    pub separator_line: bool,

    /// Abstract columns for the line containing the node.
    pub node_line: Vec<NodeLine>,

    /// Abstract columns for the line connecting the node to its parents.
    ///
    /// This is absent when the node and its parents can be represented without
    /// an explicit link line.
    pub link_line: Option<Vec<LinkLine>>,

    /// Terminator columns for anonymous parents.
    ///
    /// A `true` entry marks a column where the edge should terminate. Other
    /// columns should be rendered using the row's padding columns.
    pub term_line: Option<Vec<bool>>,

    /// Abstract columns for repeatable padding below this row.
    pub pad_lines: Vec<PadLine>,
}

/// A left-side graph prefix line before message text is attached.
///
/// This is the output of the row-shapes-to-prefix-lines stage. A prefix line
/// may contain a node glyph slot, but it does not know what glyph will fill
/// that slot and it does not contain any right-side message text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixLine {
    /// The ordered pieces that make up the graph prefix.
    pub parts: Vec<PrefixLinePart>,

    /// The semantic role of this line within the rendered row.
    pub kind: PrefixLineKind,
}

/// A piece of a graph prefix line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefixLinePart {
    /// Literal graph text.
    Text(String),

    /// Placeholder for the rendered node glyph.
    ///
    /// Keeping the glyph as a slot lets callers cache prefix lines and fill in
    /// status-dependent glyphs later.
    NodeGlyph,
}

/// The semantic role of a prefix line.
///
/// The text-output stage can use this to decide where messages should attach,
/// which lines may repeat, and which lines are meaningful for consumers that do
/// not render plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(Serialize))]
pub enum PrefixLineKind {
    /// A blank line separating disconnected one-line rows.
    Separator,

    /// A repeatable line before the node line.
    PreNode,

    /// The line containing the rendered node glyph.
    Node,

    /// A repeatable line after the node line and before edge links.
    PostNode,

    /// A line connecting the node to parent or ancestor columns.
    Link,

    /// A line terminating an anonymous parent edge.
    Term,

    /// A line continuing ancestry or parent edges.
    Ancestry,

    /// A repeatable line after ancestry continuation.
    PostAncestry,
}

impl PrefixLineKind {
    /// True if this line kind can be repeated to carry additional message lines.
    pub fn is_repeatable(self) -> bool {
        matches!(
            self,
            PrefixLineKind::PreNode | PrefixLineKind::PostNode | PrefixLineKind::PostAncestry
        )
    }
}
