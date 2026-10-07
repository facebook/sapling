/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A report file for the Python edenfsctl. The Python edenfsctl writes the
//! reason for a failed command to this file as plain text.

use std::fs::File;
use std::io::Read;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use edenfs_telemetry::cli_usage::CliUsageSample;
use tempfile::TempPath;

/// The environment variable with the report path. Keep this name the same as
/// `ERROR_REPORT_ENV` in `eden/fs/cli/error_report.py`.
const ERROR_REPORT_ENV: &str = "EDENFSCTL_ERROR_REPORT";

/// The maximum number of bytes to read from a report.
const MAX_REPORT_LEN: u64 = 64 * 1024;

/// A temporary report file. `Drop` deletes the file.
pub struct ErrorReport {
    path: TempPath,
}

impl ErrorReport {
    /// Creates an empty report file. Gives its path to `cmd` in an
    /// environment variable.
    pub fn attach(cmd: &mut Command) -> Result<Self> {
        let path = tempfile::Builder::new()
            .prefix("edenfsctl-error-")
            .tempfile()
            .context("failed to create error report file")?
            .into_temp_path();
        cmd.env(ERROR_REPORT_ENV, &path);
        Ok(Self { path })
    }

    /// Records the report in `sample` as the error message. Records nothing if
    /// the report is empty.
    pub fn record(self, sample: &mut CliUsageSample) {
        match self.text() {
            Ok(Some(text)) => sample.set_error_message(&text),
            Ok(None) => {}
            Err(error) => tracing::debug!(?error, "failed to read error report"),
        }
    }

    /// Returns the report text without leading and trailing whitespace.
    /// Returns `None` for an empty report.
    fn text(&self) -> Result<Option<String>> {
        let mut bytes = Vec::new();
        File::open(&self.path)?
            .take(MAX_REPORT_LEN)
            .read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.trim();
        Ok((!text.is_empty()).then(|| text.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn report_with(content: &str) -> Result<ErrorReport> {
        let report = ErrorReport::attach(&mut Command::new("unused"))?;
        fs::write(&report.path, content)?;
        Ok(report)
    }

    #[test]
    fn text_trims_whitespace() -> Result<()> {
        let report = report_with("CmdError: edenfs failed\nto start\n")?;

        assert_eq!(
            report.text()?.as_deref(),
            Some("CmdError: edenfs failed\nto start")
        );
        Ok(())
    }

    #[test]
    fn empty_report_has_no_text() -> Result<()> {
        assert_eq!(report_with("")?.text()?, None);
        assert_eq!(report_with(" \n\n")?.text()?, None);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn child_writes_report() -> Result<()> {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            r#"printf 'CmdError: boom' > "$EDENFSCTL_ERROR_REPORT""#,
        ]);
        let report = ErrorReport::attach(&mut cmd)?;

        assert!(cmd.status()?.success());

        assert_eq!(report.text()?.as_deref(), Some("CmdError: boom"));
        Ok(())
    }

    #[test]
    fn report_file_is_deleted_on_drop() -> Result<()> {
        let report = ErrorReport::attach(&mut Command::new("unused"))?;
        let path = report.path.to_path_buf();
        assert!(path.exists());

        drop(report);

        assert!(!path.exists(), "report file should be deleted on drop");
        Ok(())
    }
}
