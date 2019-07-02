// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::error::*;
use failure::Fallible;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct CloudSyncTrigger;

impl CloudSyncTrigger {
    pub fn fire<P: AsRef<Path>>(
        sid: &String,
        path: P,
        retries: u32,
        version: Option<u64>,
    ) -> Fallible<()> {
        let mut version_args = vec![];
        if let Some(version) = version {
            version_args.append(&mut vec![
                "--workspace-version".to_owned(),
                version.to_string(),
            ]);
        }
        for i in 0..retries {
            let now = Instant::now();
            let child = Command::new("hg")
                .current_dir(&path)
                .env("HGPLAIN", "hint")
                .args(vec!["cloud", "sync"])
                .arg("--check-autosync-enabled")
                .arg("--use-bgssh")
                .args(&version_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?; // do not retry if failed to start

            info!(
                "{} Fire `hg cloud sync` attempt {}, spawned process id '{}'",
                sid,
                i,
                child.id()
            );

            let output = child.wait_with_output()?;

            info!(
                "{} stdout: \n{}",
                sid,
                String::from_utf8_lossy(&output.stdout).trim()
            );
            info!(
                "{} stderr: \n{}",
                sid,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            let end = now.elapsed();
            info!(
                "{} Cloud Sync time: {} sec {} ms",
                sid,
                end.as_secs(),
                end.subsec_nanos() as u64 / 1_000_000
            );
            if !output.status.success() {
                error!("{} Process exited with: {}", sid, output.status);
                if i == retries - 1 {
                    return Err(ErrorKind::CommitCloudHgCloudSyncError(format!(
                        "process exited with: {}, retry later",
                        output.status
                    ))
                    .into());
                }
            } else {
                info!("{} Cloud Sync was successful", sid);
                return Ok(());
            }
        }
        Ok(())
    }
}
