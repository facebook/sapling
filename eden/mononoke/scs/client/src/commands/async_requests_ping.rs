/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Pings the async requests worker.

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use scs_client_raw::thrift;
use serde::Serialize;
use source_control::AsyncPingPollResponse;

use crate::render::Render;
use crate::ScscApp;

#[derive(Parser)]
/// List repositories
pub(super) struct CommandArgs {}

#[derive(Serialize)]
struct PingOutput {
    response: AsyncPingPollResponse,
}

impl Render for PingOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        match &self.response.result {
            Some(res) => match res {
                source_control::AsyncPingResult::success(success) => {
                    write!(w, "{}\n", success.payload)
                }
                source_control::AsyncPingResult::error(error) => {
                    write!(w, "request failed: {:?}\n", error)
                }
                source_control::AsyncPingResult::UnknownField(_) => {
                    write!(w, "unexpected result: {:?}\n", res)
                }
            },
            None => write!(w, "empty result returned\n"),
        }?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let params = thrift::AsyncPingParams {
        payload: "anything".to_string(),
        ..Default::default()
    };
    let conn = app.get_connection(None)?;

    let token = conn.async_ping(&params).await?;
    println!(
        "request sent with token {}, polling for response...",
        token.id
    );

    let res = conn.async_ping_poll(&token).await?;

    app.target
        .render_one(&args, PingOutput { response: res })
        .await
}
