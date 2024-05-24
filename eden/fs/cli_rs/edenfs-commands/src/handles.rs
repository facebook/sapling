/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl handles

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::fsutil::find_resource_locks;

use crate::ExitCode;

#[derive(Debug, Parser)]
#[clap(about = "List processes keeping a handle to a resource (such as an Eden mount)")]
pub struct HandlesCmd {
    #[clap(long, help = "The EdenFS mount point path.")]
    mount: PathBuf,
}

impl HandlesCmd {
    fn list(&self, mount: PathBuf) -> Result<ExitCode> {
        let Ok(entries) = find_resource_locks(&mount) else {
            return Ok(1);
        };
        if entries.is_empty() {
            println!(
                "No resource locks found. Typical reasons: 1) There's actually no processes holding a lock on this mount; 2) The processes are run by a different user. 3) handle.exe failed to return data, and retrying may help."
            );
            return Ok(0);
        }
        println!("Processes holding handles to the mount or its children:");
        let mut seen = HashSet::new();
        for entry in entries {
            if seen.insert(entry.process_id.clone()) {
                println!("{} ({})", entry.process_name, entry.process_id);
            }
        }
        Ok(0)
    }
}

#[async_trait]
impl crate::Subcommand for HandlesCmd {
    async fn run(&self) -> Result<ExitCode> {
        self.list(self.mount.to_owned())
    }
}
