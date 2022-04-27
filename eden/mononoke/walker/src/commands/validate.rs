/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::commands::VALIDATE;
use crate::detail::validate::{validate, ValidateCommand};
use anyhow::Error;
use clap::Parser;
use mononoke_app::MononokeApp;

use crate::args::{ValidateCheckTypeArgs, WalkerCommonArgs};
use crate::setup::setup_common;
use crate::WalkerArgs;

/// Walk the graph and perform checks on it.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    pub check_types: ValidateCheckTypeArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<(), Error> {
    let CommandArgs {
        check_types,
        common_args,
    } = args;

    let job_params = setup_common(
        VALIDATE,
        &app,
        &app.args::<WalkerArgs>()?.repos,
        &common_args,
        None, // blobstore sampler
        None, // blobstore component sampler
    )
    .await?;

    let command = ValidateCommand {
        include_check_types: check_types.parse_args(),
        progress_options: common_args.progress.parse_args(),
    };

    validate(app.fb, job_params, command).await
}
