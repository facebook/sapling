/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use hg_util::path::expand_path;

pub mod jsonrpc;

/// Expand the path if the user has supplied anything. Otherwise, use the current working directory instead.
///
/// Usage:
/// ```no_run
/// #[clap(..., parse(try_from_str = expand_path_or_cwd), default_value = "", ...)]
/// ```
pub fn expand_path_or_cwd(input: &str) -> Result<PathBuf> {
    if input.is_empty() {
        std::env::current_dir().context("Unable to retrieve current working directory")
    } else {
        Ok(expand_path(input))
    }
}
