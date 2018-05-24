use error::*;
use std::{path::Path, process::Command};

pub struct CloudSyncTrigger;

impl CloudSyncTrigger {
    pub fn fire<P: AsRef<Path>>(
        sid: &String,
        path: P,
        retries: u32,
        _version: Option<u64>,
    ) -> Result<()> {
        for i in 0..retries {
            info!(
                "{} Fire `hg cloud sync` {}",
                sid,
                if i > 0 { "retry" } else { "" }
            );
            let output = Command::new("hg")
                .current_dir(&path)
                .args(vec!["cloud", "sync"])
                .output()?;
            info!(
                "stdout: \n{}",
                String::from_utf8_lossy(&output.stdout).trim()
            );
            info!(
                "stderr: \n{}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            if !output.status.success() {
                error!("{} Process exited with: {}", sid, output.status);
                if i == retries - 1 {
                    return Err(ErrorKind::CommitCloudHgCloudSyncError(format!(
                        "process exited with: {}, retry later",
                        output.status
                    )).into());
                }
            } else {
                info!("{} Cloud Sync was successful", sid);
                return Ok(());
            }
        }
        Ok(())
    }
}
