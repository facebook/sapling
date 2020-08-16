/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;
use configparser::hg::generate_dynamicconfig;
use filetime::{set_file_mtime, FileTime};
use tempfile::tempfile_in;

define_flags! {
    pub struct DebugDynamicConfigOpts {
        /// Host name to fetch a canary config from.
        canary: Option<String>,
    }
}

pub fn run(opts: DebugDynamicConfigOpts, _io: &mut IO, repo: Repo) -> Result<u8> {
    let repo_name: String = repo
        .repo_name()
        .map_or_else(|| "".to_string(), |s| s.to_string());

    let username = repo
        .config()
        .get("ui", "username")
        .and_then(|u| Some(u.to_string()))
        .unwrap_or_else(|| "".to_string());

    generate_dynamicconfig(repo.shared_dot_hg_path(), repo_name, opts.canary, username)?;

    Ok(0)
}

pub fn name() -> &'static str {
    "debugdynamicconfig"
}

pub fn doc() -> &'static str {
    "generate the dynamic configuration"
}
