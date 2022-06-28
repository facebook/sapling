/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgEnum;
use clap::Args;
use std::collections::HashSet;
use strum_macros::AsRefStr;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

use crate::detail::graph::NodeType;

#[derive(Args, Debug)]
pub struct HashValidationArgs {
    /// Node types for which we don't want to do hash validation.
    #[clap(long)]
    pub exclude_hash_validation_node_type: Vec<HashValidationArg>,
    /// Node types for which we want to do hash validation.
    #[clap(long)]
    pub include_hash_validation_node_type: Vec<HashValidationArg>,
}

impl HashValidationArgs {
    pub fn parse_args(&self) -> HashSet<NodeType> {
        let mut include_types =
            HashValidationArg::parse_args(&self.include_hash_validation_node_type);
        let exclude_types = HashValidationArg::parse_args(&self.exclude_hash_validation_node_type);
        include_types.retain(|x| !exclude_types.contains(x));
        include_types
    }
}

#[derive(Debug, Clone, Copy, ArgEnum, AsRefStr, EnumString, EnumVariantNames)]
pub enum HashValidationArg {
    HgFileEnvelope,
}

impl HashValidationArg {
    pub fn parse_args(args: &[Self]) -> HashSet<NodeType> {
        args.iter().cloned().map(NodeType::from).collect()
    }
}

impl From<HashValidationArg> for NodeType {
    fn from(value: HashValidationArg) -> NodeType {
        match value {
            HashValidationArg::HgFileEnvelope => NodeType::HgFileEnvelope,
        }
    }
}
