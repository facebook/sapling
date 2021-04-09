/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(unix)]
use std::process::Stdio;
use std::process::{Child, Command};

use anyhow::Result;
#[cfg(windows)]
use winapi::um::winbase::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

pub fn run_background(mut command: Command) -> Result<Child> {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        command.spawn().map_err(|e| e.into())
    }
    #[cfg(unix)]
    {
        command.stderr(Stdio::null());
        command.stdout(Stdio::null());
        command.stdin(Stdio::null());
        command.spawn().map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread::sleep;
    use std::time::Duration;

    use tempdir::TempDir;

    #[test]
    fn test_basic() {
        let dir = TempDir::new("test_hgrcpath").unwrap();
        let file_path = dir.path().join("temp_file");

        #[cfg(unix)]
        let mut cmd = {
            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c")
                .arg(format!("echo foo > {}", file_path.to_string_lossy()));
            cmd
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/c")
                .arg(format!("echo foo > {}", file_path.to_string_lossy()));
            cmd
        };

        let mut child = run_background(cmd).unwrap();
        let result = child.wait().unwrap();

        assert!(file_path.exists());
    }
}
