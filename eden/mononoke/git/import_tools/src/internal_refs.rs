/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Commit cloud upload refs are a transport detail of the commit cloud client,
/// not repository state. The git server accepts the push but deliberately never
/// creates a bookmark for one, so no import path may create one either.
///
/// Matched as a prefix to cover the `upload`, `upload0` and `upload/<sha>`
/// forms clients emit.
pub const COMMIT_CLOUD_REF_PREFIX: &str = "refs/commitcloud/upload";

/// Refs that must never become Mononoke bookmarks, whichever repo or import
/// path they arrive on.
///
/// Note `refs/notes` is deliberately absent: Mononoke supports git notes as
/// first-class bookmarks via `BookmarkCategory::Note`.
pub fn is_internal_only_ref(ref_name: &[u8]) -> bool {
    ref_name.starts_with(COMMIT_CLOUD_REF_PREFIX.as_bytes())
}

#[cfg(test)]
mod tests;
