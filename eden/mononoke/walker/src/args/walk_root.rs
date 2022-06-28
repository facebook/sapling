/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::EdgeType;
use crate::detail::graph::Node;
use crate::detail::parse_node::parse_node;
use crate::detail::walk::OutgoingEdge;
use anyhow::Error;
use bookmarks::BookmarkName;
use clap::Args;

#[derive(Args, Debug)]
pub struct WalkRootArgs {
    /// Bookmark(s) to start traversal from.
    #[clap(long, short = 'b')]
    pub bookmark: Vec<BookmarkName>,
    /// Root(s) to start traversal from in format <NodeType>:<node_key>, e.g.
    /// Bookmark:master or HgChangeset:7712b62acdc858689504945ac8965a303ded6626
    #[clap(long, short = 'r')]
    pub walk_root: Vec<String>,
}

impl WalkRootArgs {
    pub fn parse_args(&self) -> Result<Vec<OutgoingEdge>, Error> {
        let mut walk_roots: Vec<OutgoingEdge> = vec![];

        let mut bookmarks = self
            .bookmark
            .iter()
            .map(|b| OutgoingEdge::new(EdgeType::RootToBookmark, Node::Bookmark(b.clone())))
            .collect();
        walk_roots.append(&mut bookmarks);

        let roots: Result<Vec<_>, Error> =
            self.walk_root.iter().map(|root| parse_node(root)).collect();
        let mut roots = roots?
            .into_iter()
            .filter_map(|node| {
                node.get_type()
                    .root_edge_type()
                    .map(|et| OutgoingEdge::new(et, node))
            })
            .collect();
        walk_roots.append(&mut roots);

        Ok(walk_roots)
    }
}
