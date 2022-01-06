/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod getbundle;
mod getpack;
mod gettreepack;
mod request;
mod util;

use crate::dispatcher::FastReplayDispatcher;
use anyhow::{Context, Error};

use getbundle::RequestGetbundleArgs;
use getpack::RequestGetpackArgs;
use gettreepack::RequestGettreepackArgs;
pub use request::RequestLine;

pub enum Request {
    Gettreepack(RequestGettreepackArgs),
    GetpackV1(RequestGetpackArgs),
    GetpackV2(RequestGetpackArgs),
    Getbundle(RequestGetbundleArgs),
}

fn parse_command_and_args(command: &str, args: &str) -> Result<Request, Error> {
    let request = match command {
        "gettreepack" => {
            Request::Gettreepack(args.parse().context("While parsing gettreepack args")?)
        }
        "getbundle" => Request::Getbundle(args.parse().context("While parsing Getbundle args")?),
        "getpackv1" => Request::GetpackV1(args.parse().context("While parsing Getpackv1 args")?),
        "getpackv2" => Request::GetpackV2(args.parse().context("While parsing Getpackv2 args")?),
        cmd @ _ => {
            return Err(Error::msg(format!("Command not supported: {}", cmd)));
        }
    };

    Ok(request)
}

pub async fn parse_request(
    req: &RequestLine<'_>,
    dispatcher: &FastReplayDispatcher,
) -> Result<Request, Error> {
    if let Some(inline_args) = &req.normal.args {
        return parse_command_and_args(&req.normal.command, inline_args);
    }

    if let Some(remote_args) = req.normal.remote_args.as_ref() {
        let remote_args = dispatcher
            .load_remote_args(remote_args.to_string())
            .await
            .context("While loading remote_args")?;
        return parse_command_and_args(&req.normal.command, &remote_args);
    }

    Err(Error::msg("No args available"))
}
