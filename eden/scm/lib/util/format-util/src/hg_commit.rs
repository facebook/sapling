/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context as _;
use anyhow::Result;
use types::Id20;

pub fn hg_commit_text_to_root_tree_id(text: &[u8]) -> Result<Id20> {
    if text.is_empty() {
        return Ok(*Id20::null_id());
    }
    let hex = text
        .get(0..Id20::hex_len())
        .context("invalid hg commit (no tree)")?;
    Ok(Id20::from_hex(hex)?)
}
