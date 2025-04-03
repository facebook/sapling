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

#[cfg(test)]
mod tests {
    use edenfs_client::utils::get_mount_point;

    use crate::utils::*;

    #[test]
    pub fn test_is_fbsource_checkout() -> anyhow::Result<()> {
        let mount_point = get_mount_point(&None)?;
        assert!(is_fbsource_checkout(&mount_point));
        Ok(())
    }
}
