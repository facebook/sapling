/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bulkops::Direction;
use clap::Args;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use std::time::Duration;

use crate::detail::checkpoint::CheckpointsByName;
use crate::detail::checkpoint::SqlCheckpoints;
use crate::detail::tail::ChunkingParams;
use crate::detail::tail::ClearStateParams;
use crate::detail::tail::TailParams;

use crate::args::arg_types::ChunkByPublicArg;
use crate::args::arg_types::InternedTypeArg;
use crate::args::arg_types::DEFAULT_INTERNED_TYPES_STR;
use crate::args::graph_arg_types::NodeTypeArg;

#[derive(Args, Debug)]
pub struct TailArgs {
    /// Max age of walk state held internally ot loaded from checkpoint that
    /// we will attempt to continue from, in seconds. Default is set to 5 days.
    // 5 days = 5 * 24 * 3600 seconds = 432000
    #[clap(long, default_value = "432000")]
    pub state_max_age: u64,
    /// Tail by polling the entry points at interval of TAIL seconds.
    #[clap(long)]
    pub tail_interval: Option<u64>,

    #[clap(flatten)]
    pub chunking: ChunkingArgs,
}

impl TailArgs {
    pub fn parse_args(
        &self,
        fb: FacebookInit,
        dbconfig: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
    ) -> Result<TailParams, Error> {
        Ok(TailParams {
            tail_secs: self.tail_interval.clone(),
            chunking: self.chunking.parse_args(fb, dbconfig, mysql_options)?,
            state_max_age: Duration::from_secs(self.state_max_age),
        })
    }
}

#[derive(Args, Debug)]
pub struct ChunkingArgs {
    /// Traverse using chunks of public changesets as roots to the specified node type
    #[clap(long, short = 'p')]
    pub chunk_by_public: Vec<ChunkByPublicArg>,
    /// Set the direction to proceed through changesets
    #[clap(long, short = 'd', requires = "chunk-by-public")]
    pub chunk_direction: Option<Direction>,
    /// How many changesets to include in a chunk.
    #[clap(long, short = 'k', default_value = "100000")]
    pub chunk_size: usize,
    /// Clear the saved walk state 1 in N steps.
    #[clap(long, short = 'K')]
    pub chunk_clear_sample_rate: Option<u64>,
    /// Whether to allow remaining deferred edges after chunks complete.
    /// Well structured repos should have none.
    #[clap(long)]
    pub allow_remaining_deferred: bool,

    /// Include in InternedTypes to flush between chunks
    #[clap(long, short = 't', default_values = &DEFAULT_INTERNED_TYPES_STR)]
    pub include_chunk_clear_interned_type: Vec<InternedTypeArg>,
    /// Exclude from InternedTypes to flush between chunks
    #[clap(long, short = 'T')]
    pub exclude_chunk_clear_interned_type: Vec<InternedTypeArg>,

    /// Include in NodeTypes to flush between chunks
    #[clap(long, short = 'n')]
    pub include_chunk_clear_node_type: Vec<NodeTypeArg>,
    /// Exclude from NodeTypes to flush between chunks
    #[clap(long, short = 'N')]
    pub exclude_chunk_clear_node_type: Vec<NodeTypeArg>,

    /// Set the repo upper bound used by chunking instead of loading it.
    /// Inclusive. Useful for reproducing issues from a particular chunk.
    #[clap(long, requires = "chunk-by-public")]
    pub repo_lower_bound: Option<u64>,
    /// Set the repo lower bound used by chunking instead of loading it.
    /// Exclusive (used in rust ranges). Useful for reproducing issues
    /// from a particular chunk.
    #[clap(long, requires = "chunk-by-public")]
    pub repo_upper_bound: Option<u64>,

    #[clap(flatten)]
    pub checkpoint: CheckpointArgs,
}

impl ChunkingArgs {
    pub fn parse_args(
        &self,
        fb: FacebookInit,
        dbconfig: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
    ) -> Result<Option<ChunkingParams>, Error> {
        if self.chunk_by_public.is_empty() {
            return Ok(None);
        }

        let clear_state = if let Some(sample_rate) = &self.chunk_clear_sample_rate {
            let mut include_int_types =
                InternedTypeArg::parse_args(&self.include_chunk_clear_interned_type);
            let exclude_int_types =
                InternedTypeArg::parse_args(&self.exclude_chunk_clear_interned_type);
            include_int_types.retain(|x| !exclude_int_types.contains(x));

            let include_nodes = NodeTypeArg::filter(
                &self.include_chunk_clear_node_type,
                &self.exclude_chunk_clear_node_type,
            );

            Some(ClearStateParams {
                sample_rate: *sample_rate,
                interned_types: include_int_types,
                node_types: include_nodes,
            })
        } else {
            None
        };

        let direction = self
            .chunk_direction
            .clone()
            .unwrap_or(Direction::NewestFirst);

        Ok(Some(ChunkingParams {
            chunk_by: ChunkByPublicArg::parse_args(&self.chunk_by_public),
            chunk_size: self.chunk_size,
            direction,
            clear_state,
            checkpoints: self.checkpoint.parse_args(fb, dbconfig, mysql_options)?,
            allow_remaining_deferred: self.allow_remaining_deferred,
            repo_lower_bound_override: self.repo_lower_bound,
            repo_upper_bound_override: self.repo_upper_bound,
        }))
    }
}

#[derive(Args, Debug)]
pub struct CheckpointArgs {
    /// Name of checkpoint.
    #[clap(long)]
    pub checkpoint_name: Option<String>,
    /// Path for sqlite checkpoint db if using sqlite
    #[clap(long, requires = "checkpoint-name")]
    pub checkpoint_path: Option<String>,
    /// Checkpoint the walk covered bounds 1 in N steps.
    #[clap(long, default_value = "1")]
    pub checkpoint_sample_rate: u64,
}

impl CheckpointArgs {
    pub fn parse_args(
        &self,
        fb: FacebookInit,
        dbconfig: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
    ) -> Result<Option<CheckpointsByName>, Error> {
        if let Some(checkpoint_name) = &self.checkpoint_name {
            let sql_checkpoints = if let Some(checkpoint_path) = &self.checkpoint_path {
                SqlCheckpoints::with_sqlite_path(checkpoint_path, false)?
            } else {
                SqlCheckpoints::with_metadata_database_config(fb, dbconfig, mysql_options, false)?
            };

            Ok(Some(CheckpointsByName::new(
                checkpoint_name.clone(),
                sql_checkpoints,
                self.checkpoint_sample_rate,
            )))
        } else {
            Ok(None)
        }
    }
}
