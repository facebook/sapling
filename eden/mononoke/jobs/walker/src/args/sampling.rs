/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use clap::Args;
use regex::Regex;

use crate::args::graph_arg_types::NodeTypeArg;
use crate::detail::sampling::SamplingOptions;

#[derive(Args, Debug)]
pub struct SamplingArgs {
    /// Pass 1 to try all nodes, 120 to do 1 in 120, etc.
    #[clap(long)]
    pub sample_rate: Option<u64>,
    /// Offset to apply to the sampling fingerprint for each node, can be used
    /// to cycle through an entire repo in N pieces.
    #[clap(long, default_value = "0")]
    pub sample_offset: u64,
    /// Node types to exclude from the sample.
    #[clap(long, short = 'S')]
    pub exclude_sample_node_type: Vec<NodeTypeArg>,
    /// Node types to include in the sample, defaults to same as the walk.
    #[clap(long, short = 's')]
    pub include_sample_node_type: Vec<NodeTypeArg>,
    /// If provided, only sample paths that match.
    #[clap(long)]
    pub sample_path_regex: Option<Regex>,
}

impl SamplingArgs {
    pub fn parse_args(&self, default_sample_rate: u64) -> Result<SamplingOptions, Error> {
        let sample_rate = self.sample_rate.clone().unwrap_or(default_sample_rate);
        let node_types = NodeTypeArg::filter(
            &self.include_sample_node_type,
            &self.exclude_sample_node_type,
        );
        let exclude_types = NodeTypeArg::parse_args(&self.exclude_sample_node_type);
        Ok(SamplingOptions {
            sample_rate,
            sample_offset: self.sample_offset,
            node_types,
            exclude_types,
        })
    }
}
