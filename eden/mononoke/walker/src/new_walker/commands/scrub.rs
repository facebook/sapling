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

use walker_commands_impl::{
    graph::Node,
    sampling::WalkSampleMapping,
    scrub::{scrub_objects, ScrubCommand, ScrubSample},
    setup::{OutputFormat, SCRUB},
};

use crate::args::{SamplingArgs, ScrubOutputNodeArgs, ScrubPackLogArgs, WalkerCommonArgs};
use crate::setup::setup_common;

#[derive(Parser)]
pub struct CommandArgs {
    /// Set the output format
    #[clap(long, short = 'F', default_value = "PrettyDebug")]
    pub output_format: OutputFormat,

    #[clap(flatten)]
    pub output_nodes: ScrubOutputNodeArgs,

    #[clap(flatten)]
    pub pack_log_info: ScrubPackLogArgs,

    #[clap(flatten, next_help_heading = "SAMPLING OPTIONS")]
    pub sampling: SamplingArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<(), Error> {
    let component_sampler = Arc::new(WalkSampleMapping::<Node, ScrubSample>::new());
    let job_params = setup_common(
        SCRUB,
        &app,
        &args.common_args,
        None,
        Some(component_sampler.clone()),
    )
    .await?;

    let CommandArgs {
        output_format,
        output_nodes,
        pack_log_info,
        sampling,
        common_args,
    } = args;
    let command = ScrubCommand {
        limit_data_fetch: common_args.limit_data_fetch,
        output_format,
        output_node_types: output_nodes.parse_args()?,
        progress_options: common_args.progress.parse_args(),
        sampling_options: sampling.parse_args(1)?,
        pack_info_log_options: pack_log_info.parse_args(app.fb)?,
        sampler: component_sampler,
    };

    scrub_objects(app.fb, job_params, command).await
}
