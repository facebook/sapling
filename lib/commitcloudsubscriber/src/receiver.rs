// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fallible;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::thread;

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
    actions: HashMap<CommandName, Box<dyn Fn() + Send>>,
}

impl TcpReceiverService {
    pub fn new(port: u16) -> TcpReceiverService {
        TcpReceiverService {
            port,
            actions: HashMap::new(),
        }
    }

    pub fn with_actions(
        mut self,
        actions: HashMap<CommandName, Box<dyn Fn() + Send>>,
    ) -> TcpReceiverService {
        self.actions = self
            .actions
            .into_iter()
            .chain(actions.into_iter())
            .collect();
        self
    }

    pub fn serve(self) -> Fallible<thread::JoinHandle<Fallible<()>>> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], self.port)))?;
        Ok(thread::spawn(move || {
            info!("Starting CommitCloud TcpReceiverService");
            info!("Listening on port {}", self.port);
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        match serde_json::from_reader::<_, Command>(stream) {
                            Ok(command) => {
                                let command_name = serde_json::to_string(&(command.0).0)
                                    .ok()
                                    .unwrap_or("unknown".into());

                                info!("Received {} command", command_name);
                                if let Some(action) = self.actions.get(&((command.0).0)) {
                                    action();
                                } else {
                                    info!("No actions found for {}", command_name);
                                }
                            }
                            Err(_) => {}
                        };
                    }
                    Err(e) => error!("Connection failed {}", e),
                }
            }
            Ok(())
        }))
    }
}
