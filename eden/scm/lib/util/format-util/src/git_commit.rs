/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context as _;
use anyhow::Result;
use types::Id20;
use types::hgid::GIT_EMPTY_TREE_ID;
use types::hgid::NULL_ID;

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

/// Resolve a git tag object to a hash.
pub fn resolve_git_tag(data: &[u8]) -> Option<Id20> {
    // See `parse_tag_buffer` in git's `tag.c`.
    let rest = data.strip_prefix(b"object ")?;
    let hex = rest.get(0..Id20::hex_len())?;
    Id20::from_hex(hex).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_tag() {
        let data = concat!(
            "object 3c05e56adf5b268ec5b20bf8aec460815e45161c\n",
            "type commit\n",
            "tag android-14.0.0_r2\n"
        );
        let resolved = resolve_git_tag(data.as_bytes());
        assert_eq!(
            resolved.unwrap().to_hex(),
            "3c05e56adf5b268ec5b20bf8aec460815e45161c"
        );
    }
}
