/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use clap::Parser;
use mononoke_app::MononokeApp;
use std::sync::Arc;

use crate::commands::COMPRESSION_BENEFIT;
use crate::detail::{
    graph::Node,
    sampling::WalkSampleMapping,
    sizing::{compression_benefit, SizingCommand, SizingSample},
};

use crate::args::{SamplingArgs, WalkerCommonArgs};
use crate::setup::setup_common;
use crate::WalkerArgs;

/// Estimate compression benefit.
#[derive(Parser)]
pub struct CommandArgs {
    /// Zstd compression level to use.
    #[clap(long, default_value = "3")]
    pub compression_level: i32,

    #[clap(flatten, next_help_heading = "SAMPLING OPTIONS")]
    pub sampling: SamplingArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<(), Error> {
    let CommandArgs {
        compression_level,
        sampling,
        common_args,
    } = args;

    let sampler = Arc::new(WalkSampleMapping::<Node, SizingSample>::new());
    let job_params = setup_common(
        COMPRESSION_BENEFIT,
        &app,
        &app.args::<WalkerArgs>()?.repos,
        &common_args,
        Some(sampler.clone()), // blobstore sampler
        None,                  // blobstore component sampler
    )
    .await?;

    let command = SizingCommand {
        compression_level,
        progress_options: common_args.progress.parse_args(),
        sampling_options: sampling.parse_args(100 /* default_sample_rate */)?,
        sampler,
    };

    compression_benefit(app.fb, job_params, command).await
}
