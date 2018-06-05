use error::*;
use std::path::Path;
use std::process::Command;

pub struct CloudSyncTrigger;

impl CloudSyncTrigger {
    pub fn fire<P: AsRef<Path>>(
        sid: &String,
        path: P,
        retries: u32,
        version: Option<u64>,
    ) -> Result<()> {
        let version_args = if let Some(version) = version {
            vec!["--workspace-version".to_owned(), version.to_string()]
        } else {
            vec![]
        };
        for i in 0..retries {
            info!(
                "{} Fire `hg cloud sync` {}",
                sid,
                if i > 0 { "retry" } else { "" }
            );
            let output = Command::new("hg")
                .current_dir(&path)
                .env("HGPLAIN", "hint")
                .args(vec!["cloud", "sync"])
                .arg("--check-autosync-enabled")
                .args(&version_args)
                .output()?;
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
