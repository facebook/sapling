/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl socket

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(about = "Print the daemon's socket path if it exists")]
pub struct SocketCmd {
    #[clap(long, help = "Print the socket path even if it doesn't exist")]
    no_check: bool,
}

#[async_trait]
impl crate::Subcommand for SocketCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let socket = instance.get_socket_path(!self.no_check);

        Ok(match socket {
            Ok(socket) => {
                print!("{}", socket.display());
                0
            }
            Err(cause) => {
                eprintln!("Error finding socket file: {}", cause);
                1
            }
        })
    }
}
