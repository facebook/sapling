/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context as _;
use anyhow::Result;
use types::hgid::GIT_EMPTY_TREE_ID;
use types::hgid::NULL_ID;
use types::Id20;

pub fn git_commit_text_to_root_tree_id(text: &[u8]) -> Result<Id20> {
    if text.is_empty() {
        return Ok(*Id20::null_id());
    }
    let hex = text
        .strip_prefix(b"tree ")
        .and_then(|t| t.get(0..Id20::hex_len()))
        .context("invalid git commit (no tree)")?;
    let id = Id20::from_hex(hex)?;
    Ok(normalize_git_tree_id(id))
}

pub(crate) fn normalize_git_tree_id(id: Id20) -> Id20 {
    if id == GIT_EMPTY_TREE_ID { NULL_ID } else { id }
}
