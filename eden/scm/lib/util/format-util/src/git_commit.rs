/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context as _;
use anyhow::Result;
use types::hgid::GIT_EMPTY_TREE_ID;
use types::hgid::NULL_ID;
use types::Id20;

use crate::utils::with_indented_commit_text;

pub fn git_commit_text_to_root_tree_id(text: &[u8]) -> Result<Id20> {
    if text.is_empty() {
        return Ok(*Id20::null_id());
    }
    let hex = text
        .strip_prefix(b"tree ")
        .and_then(|t| t.get(0..Id20::hex_len()))
        .with_context(|| {
            with_indented_commit_text(
                "invalid git commit (no tree):",
                &String::from_utf8_lossy(text),
            )
        })?;
    let id = Id20::from_hex(hex)?;
    Ok(normalize_git_tree_id(id))
}

pub(crate) fn normalize_git_tree_id(id: Id20) -> Id20 {
    if id == GIT_EMPTY_TREE_ID { NULL_ID } else { id }
}
