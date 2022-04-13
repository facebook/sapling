/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use clap::Args;
use std::collections::HashSet;
use walker_commands_impl::graph::{EdgeType, NodeType};
use walker_commands_impl::setup::{DEEP_INCLUDE_EDGE_TYPES, DEFAULT_INCLUDE_NODE_TYPES};

use crate::args::{parse_edge_types, parse_node_types};

#[derive(Args, Debug)]
pub struct WalkerGraphArgs {
    /// Graph node types to exclude from walk. They are removed from
    /// the include node types.
    // TODO: NODE_TYPE_POSSIBLE_VALUES
    #[clap(long, short = 'x')]
    pub exclude_node_type: Vec<String>,
    /// Graph node types we want to step to in the walk. Defaults to core Mononoke
    /// and Hg types.
    // TODO: NODE_TYPE_POSSIBLE_VALUES, default = DEFAULT_VALUE_ARG, also hide
    #[clap(long, short = 'i')]
    pub include_node_type: Vec<String>,

    /// Graph edge types to exclude from walk. Can pass pre-configured sets
    /// via deep, shallow, hg, bonsai, etc as well as individual types.
    // TODO: EDGE_TYPE_POSSIBLE_VALUES
    #[clap(long, short = 'X')]
    pub exclude_edge_type: Vec<String>,
    /// Graph edge types to include in the walk. Defaults to deep traversal.
    // TODO: EDGE_TYPE_POSSIBLE_VALUES, default = DEEP_VALUE_ARG, also hide
    #[clap(long, short = 'I')]
    pub include_edge_type: Vec<String>,

    /// Use this to continue walking even if walker found an error. Types of
    /// nodes to allow the walker to convert an ErrorKind::NotTraversable to
    /// a NodeData::ErrorAsData(NotTraversable)
    #[clap(long, short = 'e')]
    pub error_as_data_node_type: Vec<String>,
    /// Types of edges to allow the walker to convert an ErrorKind::NotTraversable
    /// to a NodeData::ErrorAsData(NotTraversable). If empty then allow all
    /// edges for the nodes specified via error-as-data-node-type.
    #[clap(long, short = 'E')]
    pub error_as_data_edge_type: Vec<String>,
}

pub struct WalkerGraphParams {
    pub include_node_types: HashSet<NodeType>,
    pub include_edge_types: HashSet<EdgeType>,
    pub error_as_data_node_types: HashSet<NodeType>,
    pub error_as_data_edge_types: HashSet<EdgeType>,
}

impl WalkerGraphArgs {
    pub fn parse_args(&self) -> Result<WalkerGraphParams, Error> {
        let include_node_types = parse_node_types(
            self.include_node_type.iter(),
            self.exclude_node_type.iter(),
            DEFAULT_INCLUDE_NODE_TYPES,
        )?;
        let include_edge_types = parse_edge_types(
            self.include_edge_type.iter(),
            self.exclude_edge_type.iter(),
            DEEP_INCLUDE_EDGE_TYPES,
        )?;

        let error_as_data_node_types = parse_node_types(
            self.error_as_data_node_type.iter(),
            self.exclude_node_type.iter(),
            &[],
        )?;
        let error_as_data_edge_types = parse_edge_types(
            self.error_as_data_edge_type.iter(),
            self.exclude_edge_type.iter(),
            &[],
        )?;

        Ok(WalkerGraphParams {
            include_node_types,
            include_edge_types,
            error_as_data_node_types,
            error_as_data_edge_types,
        })
    }
}
