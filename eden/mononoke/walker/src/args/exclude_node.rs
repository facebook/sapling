/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Error;
use clap::Args;

use crate::detail::graph::Node;
use crate::detail::parse_node::parse_node;

#[derive(Args, Debug)]
pub struct ExcludeNodeArgs {
    /// Nodes that should be never visited, any nodes that are reachable only via those nodes will
    /// also be omited in walks. Node format <NodeType>:<node_key>, e.g.
    /// HgChangeset:7712b62acdc858689504945ac8965a303ded6626
    #[clap(long)]
    pub exclude_node: Vec<String>,
}

impl ExcludeNodeArgs {
    pub fn parse_args(&self) -> Result<HashSet<Node>, Error> {
        self.exclude_node
            .iter()
            .map(|root| parse_node(root))
            .collect()
    }
}
