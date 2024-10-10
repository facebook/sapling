/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context as _;
use anyhow::Result;
use types::Id20;

pub fn git_commit_text_to_root_tree_id(text: &[u8]) -> Result<Id20> {
    let hex = text
        .strip_prefix(b"tree ")
        .and_then(|t| t.get(0..Id20::hex_len()))
        .context("invalid git commit")?;
    Ok(Id20::from_hex(hex)?)
}
