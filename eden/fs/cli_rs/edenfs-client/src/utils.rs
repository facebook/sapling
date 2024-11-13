/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;

/// Traverse up and locate the repository root
pub fn locate_repo_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|p| p.join(".hg").is_dir() || p.join(".git").is_dir())
}

pub fn get_mount_point(mount_point: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = mount_point {
        Ok(path.clone())
    } else {
        locate_repo_root(
            &std::env::current_dir().context("Unable to retrieve current working directory")?,
        )
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("Unable to locate repository root"))
    }
}
