/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl pid

use structopt::StructOpt;

use edenfs_client::EdenFsInstance;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
pub struct PidCmd {}

impl PidCmd {
    pub async fn run(self, instance: EdenFsInstance) -> ExitCode {
        let health = instance.get_health(None).await;

        match health {
            Ok(health) => {
                println!("{}", health.pid);
                0
            }
            Err(cause) => {
                println!("edenfs not healthy: {}", cause);
                1
            }
        }
    }
}
