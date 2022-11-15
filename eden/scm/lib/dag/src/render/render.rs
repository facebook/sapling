/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ops::Range;

use bitflags::bitflags;

use super::column::Column;
use super::column::ColumnsExt;
use super::output::OutputRendererBuilder;

pub trait Renderer<N> {
    type Output;

    // Returns the width of the graph line, possibly including another node.
    fn width(&self, new_node: Option<&N>, new_parents: Option<&Vec<Ancestor<N>>>) -> u64;

    // Reserve a column for the given node.
    fn reserve(&mut self, node: N);

    // Render the next row.
    fn next_row(
        &mut self,
        node: N,
        parents: Vec<Ancestor<N>>,
        glyph: String,
        message: String,
    ) -> Self::Output;
}

/// Renderer for a DAG.
///
/// Converts a sequence of DAG node descriptions into rendered graph rows.
pub struct GraphRowRenderer<N> {
    columns: Vec<Column<N>>,
}

/// Ancestor type indication for an ancestor or parent node.
pub enum Ancestor<N> {
    /// The node is an eventual ancestor.
    Ancestor(N),

    /// The node is an immediate parent.
    Parent(N),

    /// The node is an anonymous ancestor.
    Anonymous,
}

impl<N> Ancestor<N> {
    fn to_column(&self) -> Column<N>
    where
        N: Clone,
    {
        match self {
            Ancestor::Ancestor(n) => Column::Ancestor(n.clone()),
            Ancestor::Parent(n) => Column::Parent(n.clone()),
            Ancestor::Anonymous => Column::Blocked,
        }
    }

    fn id(&self) -> Option<&N> {
        match self {
            Ancestor::Ancestor(n) => Some(&n),
            Ancestor::Parent(n) => Some(&n),
            Ancestor::Anonymous => None,
        }
    }

    fn is_direct(&self) -> bool {
        match self {
            Ancestor::Ancestor(_) => false,
            Ancestor::Parent(_) => true,
            Ancestor::Anonymous => true,
        }
    }

    fn to_link_line(&self, direct: LinkLine, indirect: LinkLine) -> LinkLine {
        if self.is_direct() { direct } else { indirect }
    }
}

struct AncestorColumnBounds {
    target: usize,
    min_ancestor: usize,
    min_parent: usize,
    max_parent: usize,
    max_ancestor: usize,
}

impl AncestorColumnBounds {
    fn new<N>(columns: &BTreeMap<usize, &Ancestor<N>>, target: usize) -> Option<Self> {
        if columns.is_empty() {
            return None;
        }
        let min_ancestor = columns
            .iter()
            .next()
            .map_or(target, |(index, _)| *index)
            .min(target);
        let max_ancestor = columns
            .iter()
            .next_back()
            .map_or(target, |(index, _)| *index)
            .max(target);
        let min_parent = columns
            .iter()
            .find(|(_, ancestor)| ancestor.is_direct())
            .map_or(target, |(index, _)| *index)
            .min(target);
        let max_parent = columns
            .iter()
            .rev()
            .find(|(_, ancestor)| ancestor.is_direct())
            .map_or(target, |(index, _)| *index)
            .max(target);
        Some(Self {
            target,
            min_ancestor,
            min_parent,
            max_parent,
            max_ancestor,
        })
    }

    fn range(&self) -> Range<usize> {
        if self.min_ancestor < self.max_ancestor {
            self.min_ancestor + 1..self.max_ancestor
        } else {
            Default::default()
        }
    }

    fn horizontal_line(&self, index: usize) -> LinkLine {
        if index == self.target {
            LinkLine::empty()
        } else if index > self.min_parent && index < self.max_parent {
            LinkLine::HORIZ_PARENT
        } else if index > self.min_ancestor && index < self.max_ancestor {
            LinkLine::HORIZ_ANCESTOR
        } else {
            LinkLine::empty()
        }
    }
}

impl<N> Column<N> {
    fn to_node_line(&self) -> NodeLine {
        match self {
            Column::Ancestor(_) => NodeLine::Ancestor,
            Column::Parent(_) => NodeLine::Parent,
            _ => NodeLine::Blank,
        }
    }

    fn to_link_line(&self) -> LinkLine {
        match self {
            Column::Ancestor(_) => LinkLine::VERT_ANCESTOR,
            Column::Parent(_) => LinkLine::VERT_PARENT,
            _ => LinkLine::empty(),
        }
    }

    fn to_pad_line(&self) -> PadLine {
        match self {
            Column::Ancestor(_) => PadLine::Ancestor,
            Column::Parent(_) => PadLine::Parent,
            _ => PadLine::Blank,
        }
    }
}

/// A column in the node row.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeLine {
    /// Blank.
    Blank,

    /// Vertical line indicating an ancestor.
    Ancestor,

    /// Vertical line indicating a parent.
    Parent,

    /// The node for this row.
    Node,
}

/// A column in a padding row.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PadLine {
    /// Blank.
    Blank,

    /// Vertical line indicating an ancestor.
    Ancestor,

    /// Vertical line indicating a parent.
    Parent,
}

bitflags! {
    /// A column in a linking row.
    #[derive(Default)]
    pub struct LinkLine: u16 {
        /// This cell contains a horizontal line that connects to a parent.
        const HORIZ_PARENT = 0b0_0000_0000_0001;

        /// This cell contains a horizontal line that connects to an ancestor.
        const HORIZ_ANCESTOR = 0b0_0000_0000_0010;

        /// The descendent of this cell is connected to the parent.
        const VERT_PARENT = 0b0_0000_0000_0100;

        /// The descendent of this cell is connected to an ancestor.
        const VERT_ANCESTOR = 0b0_0000_0000_1000;

        /// The parent of this cell is linked in this link row and the child
        /// is to the left.
        const LEFT_FORK_PARENT = 0b0_0000_0001_0000;

        /// The ancestor of this cell is linked in this link row and the child
        /// is to the left.
        const LEFT_FORK_ANCESTOR = 0b0_0000_0010_0000;

        /// The parent of this cell is linked in this link row and the child
        /// is to the right.
        const RIGHT_FORK_PARENT = 0b0_0000_0100_0000;

        /// The ancestor of this cell is linked in this link row and the child
        /// is to the right.
        const RIGHT_FORK_ANCESTOR = 0b0_0000_1000_0000;

        /// The child of this cell is linked to parents on the left.
        const LEFT_MERGE_PARENT = 0b0_0001_0000_0000;

        /// The child of this cell is linked to ancestors on the left.
        const LEFT_MERGE_ANCESTOR = 0b0_0010_0000_0000;

        /// The child of this cell is linked to parents on the right.
        const RIGHT_MERGE_PARENT = 0b0_0100_0000_0000;

        /// The child of this cell is linked to ancestors on the right.
        const RIGHT_MERGE_ANCESTOR = 0b0_1000_0000_0000;

        /// The target node of this link line is the child of this column.
        /// This disambiguates between the node that is connected in this link
        /// line, and other nodes that are also connected vertically.
        const CHILD = 0b1_0000_0000_0000;

        const HORIZONTAL = Self::HORIZ_PARENT.bits | Self::HORIZ_ANCESTOR.bits;
        const VERTICAL = Self::VERT_PARENT.bits | Self::VERT_ANCESTOR.bits;
        const LEFT_FORK = Self::LEFT_FORK_PARENT.bits | Self::LEFT_FORK_ANCESTOR.bits;
        const RIGHT_FORK = Self::RIGHT_FORK_PARENT.bits | Self::RIGHT_FORK_ANCESTOR.bits;
        const LEFT_MERGE = Self::LEFT_MERGE_PARENT.bits | Self::LEFT_MERGE_ANCESTOR.bits;
        const RIGHT_MERGE = Self::RIGHT_MERGE_PARENT.bits | Self::RIGHT_MERGE_ANCESTOR.bits;
        const ANY_MERGE = Self::LEFT_MERGE.bits | Self::RIGHT_MERGE.bits;
        const ANY_FORK = Self::LEFT_FORK.bits | Self::RIGHT_FORK.bits;
        const ANY_FORK_OR_MERGE = Self::ANY_MERGE.bits | Self::ANY_FORK.bits;
    }
}

/// An output graph row.
#[derive(Debug)]
pub struct GraphRow<N> {
    /// The name of the node for this row.
    pub node: N,

    /// The glyph for this node.
    pub glyph: String,

    /// The message for this row.
    pub message: String,

    /// True if this row is for a merge commit.
    pub merge: bool,

    /// The node columns for this row.
    pub node_line: Vec<NodeLine>,

    /// The link columns for this row, if a link row is necessary.
    pub link_line: Option<Vec<LinkLine>>,

    /// The location of any terminators, if necessary.  Other columns should be
    /// filled in with pad lines.
    pub term_line: Option<Vec<bool>>,

    /// The pad columns for this row.
    pub pad_lines: Vec<PadLine>,
}

impl<N> GraphRowRenderer<N>
where
    N: Clone + Eq,
{
    /// Create a new renderer.
    pub fn new() -> Self {
        GraphRowRenderer {
            columns: Vec::new(),
        }
    }

    /// Build an output renderer from this renderer.
    pub fn output(self) -> OutputRendererBuilder<N, Self> {
        OutputRendererBuilder::new(self)
    }
}

impl<N> Renderer<N> for GraphRowRenderer<N>
where
    N: Clone + Eq,
{
    type Output = GraphRow<N>;

    fn width(&self, node: Option<&N>, parents: Option<&Vec<Ancestor<N>>>) -> u64 {
        let mut width = self.columns.len();
        let mut empty_columns = self
            .columns
            .iter()
            .filter(|&column| column == &Column::Empty)
            .count();
        if let Some(node) = node {
            // If the node is not already allocated, and there is no
            // space for the node, then adding the new node would create
            // a new column.
            if self.columns.find(&node).is_none() {
                if empty_columns == 0 {
                    width += 1;
                } else {
                    empty_columns = empty_columns.saturating_sub(1);
                }
            }
        }
        if let Some(parents) = parents {
            // Non-allocated parents will also need a new column (except
            // for one, which can take the place of the node, and any that could be allocated to
            // empty columns).
            let unallocated_parents = parents
                .iter()
                .filter(|parent| {
                    parent
                        .id()
                        .map_or(true, |parent| self.columns.find(&parent).is_none())
                })
                .count()
                .saturating_sub(empty_columns);
            width += unallocated_parents.saturating_sub(1);
        }
        width as u64
    }

    fn reserve(&mut self, node: N) {
        if self.columns.find(&node).is_none() {
            if let Some(index) = self.columns.first_empty() {
                self.columns[index] = Column::Reserved(node);
            } else {
                self.columns.push(Column::Reserved(node));
            }
        }
    }

    fn next_row(
        &mut self,
        node: N,
        parents: Vec<Ancestor<N>>,
        glyph: String,
        message: String,
    ) -> GraphRow<N> {
        // Find a column for this node.
        let column = self.columns.find(&node).unwrap_or_else(|| {
            self.columns
                .first_empty()
                .unwrap_or_else(|| self.columns.new_empty())
        });
        self.columns[column] = Column::Empty;

        // This row is for a merge if there are multiple parents.
        let merge = parents.len() > 1;

        // Build the initial node line.
        let mut node_line: Vec<_> = self.columns.iter().map(|c| c.to_node_line()).collect();
        node_line[column] = NodeLine::Node;

        // Build the initial link line.
        let mut link_line: Vec<_> = self.columns.iter().map(|c| c.to_link_line()).collect();
        let mut need_link_line = false;

        // Build the initial term line.
        let mut term_line: Vec<_> = self.columns.iter().map(|_| false).collect();
        let mut need_term_line = false;

        // Build the initial pad line.
        let mut pad_lines: Vec<_> = self.columns.iter().map(|c| c.to_pad_line()).collect();

        // Assign each parent to a column.
        let mut parent_columns = BTreeMap::new();
        for p in parents.iter() {
            // Check if the parent already has a column.
            if let Some(parent_id) = p.id() {
                if let Some(index) = self.columns.find(parent_id) {
                    self.columns[index].merge(&p.to_column());
                    parent_columns.insert(index, p);
                    continue;
                }
            }
            // Assign the parent to an empty column, preferring the column
            // the current node is going in, to maintain linearity.
            if let Some(index) = self.columns.find_empty(column) {
                self.columns[index].merge(&p.to_column());
                parent_columns.insert(index, p);
                continue;
            }
            // There are no empty columns left.  Make a new column.
            parent_columns.insert(self.columns.len(), p);
            node_line.push(NodeLine::Blank);
            pad_lines.push(PadLine::Blank);
            link_line.push(LinkLine::default());
            term_line.push(false);
            self.columns.push(p.to_column());
        }

        // Mark parent columns with anonymous parents as terminating.
        for (i, p) in parent_columns.iter() {
            if p.id().is_none() {
                term_line[*i] = true;
                need_term_line = true;
            }
        }

        // Check if we can move the parent to the current column.
        if parents.len() == 1 {
            if let Some((&parent_column, _)) = parent_columns.iter().next() {
                if parent_column > column {
                    // This node has a single parent which was already
                    // assigned to a column to the right of this one.
                    // Move the parent to this column.
                    self.columns.swap(column, parent_column);
                    let parent = parent_columns
                        .remove(&parent_column)
                        .expect("parent should exist");
                    parent_columns.insert(column, parent);

                    // Generate a line from this column to the old
                    // parent column.   We need to continue with the style
                    // that was being used for the parent column.
                    let was_direct = link_line[parent_column].contains(LinkLine::VERT_PARENT);
                    link_line[column] |= if was_direct {
                        LinkLine::RIGHT_FORK_PARENT
                    } else {
                        LinkLine::RIGHT_FORK_ANCESTOR
                    };
                    #[allow(clippy::needless_range_loop)]
                    for i in column + 1..parent_column {
                        link_line[i] |= if was_direct {
                            LinkLine::HORIZ_PARENT
                        } else {
                            LinkLine::HORIZ_ANCESTOR
                        };
                    }
                    link_line[parent_column] = if was_direct {
                        LinkLine::LEFT_MERGE_PARENT
                    } else {
                        LinkLine::LEFT_MERGE_ANCESTOR
                    };
                    need_link_line = true;
                    // The pad line for the old parent column is now blank.
                    pad_lines[parent_column] = PadLine::Blank;
                }
            }
        }

        // Connect the node column to all the parent columns.
        if let Some(bounds) = AncestorColumnBounds::new(&parent_columns, column) {
            // If the parents extend beyond the columns adjacent to the node, draw a horizontal
            // ancestor line between the two outermost ancestors.
            for i in bounds.range() {
                link_line[i] |= bounds.horizontal_line(i);
                need_link_line = true;
            }

            // If there is a parent or ancestor to the right of the node
            // column, the node merges from the right.
            if bounds.max_parent > column {
                link_line[column] |= LinkLine::RIGHT_MERGE_PARENT;
                need_link_line = true;
            } else if bounds.max_ancestor > column {
                link_line[column] |= LinkLine::RIGHT_MERGE_ANCESTOR;
                need_link_line = true;
            }

            // If there is a parent or ancestor to the left of the node column, the node merges from the left.
            if bounds.min_parent < column {
                link_line[column] |= LinkLine::LEFT_MERGE_PARENT;
                need_link_line = true;
            } else if bounds.min_ancestor < column {
                link_line[column] |= LinkLine::LEFT_MERGE_ANCESTOR;
                need_link_line = true;
            }

            // Each parent or ancestor forks towards the node column.
            #[allow(clippy::comparison_chain)]
            for (&i, p) in parent_columns.iter() {
                pad_lines[i] = self.columns[i].to_pad_line();
                if i < column {
                    link_line[i] |=
                        p.to_link_line(LinkLine::RIGHT_FORK_PARENT, LinkLine::RIGHT_FORK_ANCESTOR);
                } else if i == column {
                    link_line[i] |= LinkLine::CHILD
                        | p.to_link_line(LinkLine::VERT_PARENT, LinkLine::VERT_ANCESTOR);
                } else {
                    link_line[i] |=
                        p.to_link_line(LinkLine::LEFT_FORK_PARENT, LinkLine::LEFT_FORK_ANCESTOR);
                }
            }
        }

        // Now that we have assigned all the columns, reset their state.
        self.columns.reset();

        // Filter out the link line or term line if they are not needed.
        let link_line = Some(link_line).filter(|_| need_link_line);
        let term_line = Some(term_line).filter(|_| need_term_line);

        GraphRow {
            node,
            glyph,
            message,
            merge,
            node_line,
            link_line,
            term_line,
            pad_lines,
        }
    }
}
