/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod progress;
pub mod sampling;
pub mod scrub;
pub mod tail_args;
pub mod walk_params;
pub mod walk_root;

pub use progress::ProgressArgs;
pub use sampling::SamplingArgs;
pub use scrub::{ScrubOutputNodeArgs, ScrubPackLogArgs};
pub use tail_args::{CheckpointArgs, ChunkingArgs, TailArgs};
pub use walk_params::{HashValidationArgs, WalkerGraphArgs, WalkerGraphParams};
pub use walk_root::WalkRootArgs;

use anyhow::Error;
use clap::Args;
use itertools::{process_results, Itertools};
use std::collections::HashSet;
use walker_commands_impl::graph::{EdgeType, NodeType};
use walker_commands_impl::setup::{parse_edge_value, parse_interned_value, parse_node_value};
use walker_commands_impl::state::InternedType;

#[derive(Args, Debug)]
pub struct WalkerCommonArgs {
    /// Log a lot less
    #[clap(long, short = 'q')]
    pub quiet: bool,
    /// Use redaction from config. Default is redaction off.
    #[clap(long)]
    pub enable_redaction: bool,
    /// Maximum number of walk step tasks to attempt to execute at once.
    #[clap(long, default_value = "4096")]
    pub scheduled_max: usize,
    /// Enable derivation of data (e.g. hg, file metadata).
    #[clap(long)]
    pub enable_derive: bool,
    /// Limit the amount of data fetched from stores, by not streaming
    /// large files to the end. Only used by `scrub` subcommand.
    #[clap(long)]
    pub limit_data_fetch: bool,

    /// Id of a storage group to operate over, e.g. manifold_xdb_multiplex
    pub storage_id: Option<String>,
    /// If main blobstore in the storage config is a multiplexed one,
    /// use inner blobstore with this id.
    #[clap(long)]
    pub inner_blobstore_id: Option<u64>,
    /// Add a multiplier on sampling requests
    #[clap(long, default_value = "100")]
    pub blobstore_sampling_multiplier: u64,

    #[clap(flatten, next_help_heading = "WALKING ROOTS")]
    pub walk_roots: WalkRootArgs,
    #[clap(flatten, next_help_heading = "GRAPH OPTIONS")]
    pub graph_params: WalkerGraphArgs,
    #[clap(flatten, next_help_heading = "HASH VALIDATION OPTIONS")]
    pub hash_validation: HashValidationArgs,
    #[clap(flatten, next_help_heading = "PROGRESS OPTIONS")]
    pub progress: ProgressArgs,
    #[clap(flatten, next_help_heading = "TAILING OPTIONS")]
    pub tailing: TailArgs,
}

fn parse_node_types<'a>(
    include_node_types: impl Iterator<Item = &'a String>,
    exclude_node_types: impl Iterator<Item = &'a String>,
    default: &[NodeType],
) -> Result<HashSet<NodeType>, Error> {
    let mut include_node_types = parse_node_values(include_node_types, default)?;
    let exclude_node_types = parse_node_values(exclude_node_types, &[])?;
    include_node_types.retain(|x| !exclude_node_types.contains(x));
    Ok(include_node_types)
}

fn parse_node_values<'a>(
    values: impl Iterator<Item = &'a String>,
    default: &[NodeType],
) -> Result<HashSet<NodeType>, Error> {
    let node_values = process_results(values.map(|x| parse_node_value(x)), |s| s.concat())?;

    if node_values.is_empty() {
        return Ok(HashSet::from_iter(default.iter().cloned()));
    }
    Ok(node_values)
}

fn parse_interned_types<'a>(
    include_types: impl Iterator<Item = &'a String>,
    exclude_types: impl Iterator<Item = &'a String>,
    default: &[InternedType],
) -> Result<HashSet<InternedType>, Error> {
    let mut include_types = parse_interned_values(include_types, default)?;
    let exclude_types = parse_interned_values(exclude_types, &[])?;
    include_types.retain(|x| !exclude_types.contains(x));
    Ok(include_types)
}

fn parse_interned_values<'a>(
    values: impl Iterator<Item = &'a String>,
    default: &[InternedType],
) -> Result<HashSet<InternedType>, Error> {
    let values = process_results(values.map(|v| parse_interned_value(v, default)), |s| {
        s.concat()
    })?;

    if values.is_empty() {
        return Ok(default.iter().cloned().collect());
    }
    Ok(values)
}

pub(crate) fn parse_edge_types<'a>(
    include_types: impl ExactSizeIterator<Item = &'a String>,
    exclude_types: impl ExactSizeIterator<Item = &'a String>,
    default: &[EdgeType],
) -> Result<HashSet<EdgeType>, Error> {
    let mut include_edge_types = parse_edge_values(include_types, default)?;
    let exclude_edge_types = parse_edge_values(exclude_types, &[])?;
    include_edge_types.retain(|x| !exclude_edge_types.contains(x));
    Ok(include_edge_types)
}

fn parse_edge_values<'a>(
    values: impl ExactSizeIterator<Item = &'a String>,
    default: &[EdgeType],
) -> Result<HashSet<EdgeType>, Error> {
    if values.len() == 0 {
        Ok(default.iter().cloned().collect())
    } else {
        process_results(values.map(|v| parse_edge_value(v)), |s| s.concat())
    }
}
