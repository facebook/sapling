/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod arg_types;
mod graph_arg_types;
pub mod hash_validation;
pub mod progress;
pub mod sampling;
pub mod scrub;
pub mod tail_args;
pub mod validate;
pub mod walk_params;
pub mod walk_root;

pub use graph_arg_types::NodeTypeArg;
pub use hash_validation::HashValidationArgs;
pub use progress::ProgressArgs;
pub use sampling::SamplingArgs;
pub use scrub::ScrubOutputNodeArgs;
pub use scrub::ScrubPackLogArgs;
pub use tail_args::CheckpointArgs;
pub use tail_args::ChunkingArgs;
pub use tail_args::TailArgs;
pub use validate::ValidateCheckTypeArgs;
pub use walk_params::WalkerGraphArgs;
pub use walk_params::WalkerGraphParams;
pub use walk_root::WalkRootArgs;

use clap::Args;
use strum_macros::AsRefStr;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

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
    #[clap(long)]
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

#[derive(Clone, Debug, PartialEq, Eq, AsRefStr, EnumVariantNames, EnumString)]
pub enum OutputFormat {
    Debug,
    PrettyDebug,
}
