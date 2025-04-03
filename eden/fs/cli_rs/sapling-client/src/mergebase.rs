/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use tokio::process::Command;

use crate::get_sapling_executable_path;
use crate::get_sapling_options;

pub async fn get_mergebase(commit: &str, mergegase_with: &str) -> anyhow::Result<Option<String>> {
    let output = Command::new(get_sapling_executable_path())
        .envs(get_sapling_options())
        .args([
            "log",
            "-T",
            "{node}",
            "-r",
            format!("ancestor({}, {})", commit, mergegase_with).as_str(),
        ])
        .output()
        .await?;

    let mergebase = String::from_utf8(output.stdout)?;
    if mergebase.is_empty() {
        Ok(None)
    } else {
        Ok(Some(mergebase))
    }
}
