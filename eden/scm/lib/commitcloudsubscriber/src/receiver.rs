/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::marker::Unpin;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use log::error;
use log::info;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::ActionsMap;

/// Set of supported commands
/// All unknown commands will be ignored
#[derive(Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum CommandName {
    #[serde(rename = "commitcloud::restart_subscriptions")]
    CommitCloudRestartSubscriptions,
    #[serde(rename = "commitcloud::cancel_subscriptions")]
    CommitCloudCancelSubscriptions,
    #[serde(rename = "commitcloud::start_subscriptions")]
    CommitCloudStartSubscriptions,
}

#[derive(Debug, Deserialize, Default, Serialize)]
pub struct CommandData {
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Command(pub (CommandName, CommandData));

/// Simple cross platform commands receiver working on top of Tcp Socket and json
/// Expected commands are in json format
/// Example: ["commitcloud::restart_subscriptions", {"foo": "bar"}]
/// Example to test: echo '["commitcloud::restart_subscriptions", {}]' | nc localhost 15432
/// with_actions builder is used to configure callbacks
/// The serve function starts the service

pub struct TcpReceiverService {
    port: u16,
    actions: Arc<ActionsMap>,
}

impl TcpReceiverService {
    pub fn new(port: u16, actions: ActionsMap) -> TcpReceiverService {
        TcpReceiverService {
            port,
            actions: Arc::new(actions),
        }
    }

    async fn handler(actions: Arc<ActionsMap>, mut socket: impl AsyncRead + Unpin) -> Result<()> {
        let mut buf = Vec::new();
        let bytes_read = socket.read_to_end(&mut buf).await?;

        if bytes_read == 0 {
            // Ping connection, client checks if scm daemon is alive
            // TODO: implement proper health_check request
            return Ok(());
        }

        let command: Command = serde_json::from_slice(&buf[..bytes_read])?;
        let command_name = serde_json::to_string(&(command.0).0)
            .ok()
            .unwrap_or("unknown".into());

        info!("Received {} command", command_name);

        match actions.get(&((command.0).0)) {
            Some(action) => action(),
            None => info!("No actions found for {}", command_name),
        }

        Ok(())
    }

    pub fn serve(self) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            info!("Starting CommitCloud TcpReceiverService");
            let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], self.port))).await?;
            info!("Listening on port {}", self.port);
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let actions = self.actions.clone();
                        tokio::spawn(async move {
                            if let Err(err) = Self::handler(actions, socket).await {
                                error!("Failed to handle connection: {err}")
                            }
                        });
                    }
                    Err(err) => error!("{err}"),
                }
            }
        })
    }
}
