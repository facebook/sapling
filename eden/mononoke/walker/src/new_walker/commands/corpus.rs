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
    corpus::{corpus, CorpusCommand, CorpusSample, CorpusSamplingHandler},
    setup::CORPUS,
};

use crate::args::{SamplingArgs, WalkerCommonArgs};
use crate::setup::setup_common;
use crate::WalkerArgs;

/// Dump a sampled corpus of blobstore data.
#[derive(Parser)]
pub struct CommandArgs {
    /// Where to write the output corpus. Default is to to a dry run with no output.
    #[clap(long)]
    pub output_dir: Option<String>,

    #[clap(flatten, next_help_heading = "SAMPLING OPTIONS")]
    pub sampling: SamplingArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<(), Error> {
    let CommandArgs {
        output_dir,
        sampling,
        common_args,
    } = args;

    let sampler = Arc::new(CorpusSamplingHandler::<CorpusSample>::new(
        output_dir.clone(),
    ));
    let job_params = setup_common(
        CORPUS,
        &app,
        &app.args::<WalkerArgs>()?.repos,
        &common_args,
        Some(sampler.clone()), // blobstore sampler
        None,                  // blobstore component sampler
    )
    .await?;

    if let Some(output_dir) = &output_dir {
        if !std::path::Path::new(output_dir).exists() {
            std::fs::create_dir(output_dir).map_err(Error::from)?
        }
    }

    let command = CorpusCommand {
        output_dir,
        progress_options: common_args.progress.parse_args(),
        sampling_options: sampling.parse_args(100 /* default_sample_rate */)?,
        sampling_path_regex: sampling.sample_path_regex.clone(),
        sampler,
    };

    corpus(app.fb, job_params, command).await
}
