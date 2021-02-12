/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use sysinfo::{ProcessExt, SystemExt};

pub fn get_executable(pid: sysinfo::Pid) -> Option<PathBuf> {
    let mut system = sysinfo::System::new();

    if !system.refresh_process(pid) {
        if let Some(process) = system.get_process(pid) {
            let executable = process.exe();

            #[cfg(unix)]
            {
                // We may get a path ends with (deleted) if the executable is deleted on UNIX.
                let path = executable
                    .to_str()
                    .unwrap_or("")
                    .trim_end_matches(" (deleted)");
                return Some(path.into());
            }
            #[cfg(not(unix))]
            {
                return Some(executable.into());
            }
        }
    }

    None
}
