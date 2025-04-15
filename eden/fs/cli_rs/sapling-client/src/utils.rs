/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::read_to_string;
use std::path::Path;

use crate::types::SaplingStatus;

pub(crate) fn get_sapling_executable_path() -> String {
    let path = env::var("EDEN_HG_BINARY").unwrap_or_else(|_| String::new());
    if path.is_empty() {
        "hg".to_string() // `sl` is not always available, so use `hg`
    } else {
        path
    }
}

pub(crate) fn get_sapling_options() -> HashMap<OsString, OsString> {
    let mut options = HashMap::<OsString, OsString>::new();
    // Ensure that the hgrc doesn't mess with the behavior of the commands that we're running.
    options.insert("HGPLAIN".to_string().into(), "1".to_string().into());
    // Ensure that we do not log profiling data for the commands we are
    // running. This is to avoid a significant increase in the rate of logging.
    options.insert("NOSCMLOG".to_string().into(), "1".to_string().into());
    // chg can elect to kill all children if an error occurs in any child.
    // This can cause commands we spawn to fail transiently.  While we'd
    // love to have the lowest latency, the transient failure causes problems
    // with our ability to deliver notifications to our clients in a timely
    // manner, so we disable the use of chg for the sapling processes
    // that we spawn.
    options.insert("CHGDISABLE".to_string().into(), "1".to_string().into());
    options
}

pub fn is_fbsource_checkout(mount_point: &Path) -> bool {
    let project_id_path = mount_point.join(".projectid");
    let project_id = read_to_string(project_id_path).ok();
    match project_id {
        Some(project_id) => project_id.trim() == "fbsource",
        None => false,
    }
}

//
// Single line looks like:
//    <status> <path>
//
// Where status is one of:
//   M = modified
//   A = added
//   R = removed
//   C = clean
//   ! = missing (deleted by a non-sl command, but still tracked)
//   ? = not tracked
//   I = ignored
//     = origin of the previous file (with --copies)
// Note:
//   Paths can have spaces, but are not quoted.
pub(crate) fn process_one_status_line(
    line: &str,
) -> anyhow::Result<Option<(SaplingStatus, String)>> {
    // Must include a status and at least one char path.
    let mut chars = line.chars();
    let status = chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid status line: {line}"))?;
    let space = chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid status line: {line}"))?;
    if space != ' ' {
        return Err(anyhow::anyhow!("Invalid status line: {line}"));
    }
    let path = line.chars().skip(1).collect::<String>().trim().to_owned();
    let result = match status {
        'M' => Some((SaplingStatus::Modified, path)),
        'A' => Some((SaplingStatus::Added, path)),
        'R' => Some((SaplingStatus::Removed, path)),
        'C' => Some((SaplingStatus::Clean, path)),
        '!' => Some((SaplingStatus::Missing, path)),
        '?' => Some((SaplingStatus::NotTracked, path)),
        'I' => Some((SaplingStatus::Ignored, path)),
        ' ' => Some((SaplingStatus::Copied, path)),
        _ => None, // Skip all others
    };

    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Error;
    use std::io::ErrorKind;

    use async_process_traits::MockChild;
    use async_process_traits::MockChildHandle;
    use async_process_traits::MockCommandSpawner;
    use async_process_traits::MockExitStatus;
    use edenfs_client::utils::get_mount_point;
    use tokio::io;
    use tokio_test::io::Builder as MockIoBuilder;

    use crate::types::*;
    use crate::utils::*;

    #[test]
    pub fn test_is_fbsource_checkout() -> anyhow::Result<()> {
        let mount_point = get_mount_point(&None)?;
        assert!(is_fbsource_checkout(&mount_point));
        Ok(())
    }

    #[test]
    fn test_process_one_status_line() -> anyhow::Result<()> {
        assert_eq!(
            process_one_status_line("M buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Modified,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("A buck2/app/buck2_file_watcher/src/edenfs/interface.rs")?,
            Some((
                SaplingStatus::Added,
                "buck2/app/buck2_file_watcher/src/edenfs/interface.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("R buck2/app/buck2_file_watcher/src/edenfs/utils.rs")?,
            Some((
                SaplingStatus::Removed,
                "buck2/app/buck2_file_watcher/src/edenfs/utils.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("! buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Missing,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("? buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::NotTracked,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("C buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Clean,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("I buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Ignored,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("  buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Copied,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        // Space in path
        assert_eq!(
             process_one_status_line("M ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs")?,
             Some((
                 SaplingStatus::Modified,
                 "ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs".to_owned()
             ))
         );

        // Invalid status
        assert!(process_one_status_line("Invalid status").is_err());

        // Invalid status (missing status), but valid path with space in it
        assert!(
             process_one_status_line(" ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs")
             .is_err());

        // Malformed status (no space)
        assert!(
            process_one_status_line("Mbuck2/app/buck2_file_watcher/src/edenfs/sapling.rs").is_err()
        );

        // Malformed status (colon instead of space)
        assert!(
            process_one_status_line("M:buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")
                .is_err()
        );

        Ok(())
    }

    pub(crate) fn get_mock_spawner(
        program: String,
        output: Option<(i32, Option<Vec<u8>>)>,
    ) -> MockCommandSpawner {
        MockCommandSpawner::with_callback(move |cmd| match cmd.program.to_str() {
            Some(cmd_program) if cmd_program == program && output.is_some() => {
                let (exit_code, stdout_lines) = output.clone().unwrap();
                Ok(mock_child(exit_code, stdout_lines))
            }
            program => Err(Error::new(
                ErrorKind::Other,
                anyhow::anyhow!("Not expected program: {:?}", program),
            )),
        })
    }

    fn mock_child(exit_code: i32, stdout_lines: Option<Vec<u8>>) -> MockChild {
        let handle = MockChildHandle::new();
        handle.set_status(Ok(Some(MockExitStatus::new(Some(exit_code)))));
        if let Some(stdout_lines) = stdout_lines {
            let stdout = MockIoBuilder::new().read(&stdout_lines).build();
            MockChild::with_stdio(handle, Some(io::sink()), Some(stdout), Some(io::empty()))
        } else {
            MockChild::new(handle)
        }
    }
}
