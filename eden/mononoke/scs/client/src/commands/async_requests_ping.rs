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
use source_control_clients::errors::AsyncPingPollError;

use crate::render::Render;
use crate::ScscApp;

#[derive(Parser)]
/// List repositories
pub(super) struct CommandArgs {}

#[derive(Serialize)]
struct PingOutput {
    response: thrift::AsyncPingResponse,
}

impl Render for PingOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "received: {}", self.response.payload)?;
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

    let response = {
        loop {
            let res = conn.async_ping_poll(&token).await;
            match res {
                Ok(res) => match res {
                    source_control::AsyncPingPollResponse::response(success) => {
                        break success;
                    }
                    source_control::AsyncPingPollResponse::poll_pending(_) => {
                        println!("ping is not ready yet, waiting some more...");
                    }
                    source_control::AsyncPingPollResponse::UnknownField(t) => {
                        return Err(anyhow::anyhow!(
                            "request failed with unknown result: {:?}",
                            t
                        ));
                    }
                },
                Err(e) => match e {
                    AsyncPingPollError::poll_error(_) => {
                        // retry
                    }
                    _ => return Err(anyhow::anyhow!("request failed with error: {:?}", e)),
                },
            }
        }
    };

    app.target.render_one(&args, PingOutput { response }).await
}
