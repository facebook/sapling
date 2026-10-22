/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Three-way merge for text files.
//!
//! Provides a content-level 3-way merge algorithm matching Git's auto-merge
//! behavior. Given three versions of a file (base, local, other), determines
//! whether the changes from both sides can be combined without conflicts.
//!
//! Uses xdiff (the same diff engine as Git) to compute matching blocks,
//! then applies the classic "sync regions" algorithm to classify and
//! merge the gaps.

mod merge3;
pub mod utils;

/// The result of a 3-way merge attempt.
#[derive(Debug)]
pub enum MergeResult {
    /// Both sides' changes were compatible. Contains the merged bytes.
    Clean(Vec<u8>),
    /// A conflict was detected. Contains a human-readable description.
    Conflict(String),
}

/// Attempt a 3-way merge of text file content.
///
/// Given the base (common ancestor), local (our changes), and other (their
/// changes) versions of a file, attempts to produce a merged result.
///
/// Returns `MergeResult::Clean(bytes)` if the merge succeeds, or
/// `MergeResult::Conflict(description)` if the changes are incompatible.
///
/// **Binary files**: If any of the three inputs contains a null byte,
/// and the inputs are not all identical, this returns a conflict.
pub fn merge_text(base: &[u8], local: &[u8], other: &[u8]) -> MergeResult {
    // Fast path: if all three are identical, no merge needed.
    if base == local && base == other {
        return MergeResult::Clean(base.to_vec());
    }

    // If only one side changed, take the changed side.
    if base == local {
        return MergeResult::Clean(other.to_vec());
    }
    if base == other {
        return MergeResult::Clean(local.to_vec());
    }

    // Both sides differ from base. Check for binary content.
    if utils::is_binary(base) || utils::is_binary(local) || utils::is_binary(other) {
        return MergeResult::Conflict("binary file changed on both sides".to_string());
    }

    // Both sides changed — run the line-level merge algorithm.
    match merge3::merge3(base, local, other) {
        Ok(merged) => MergeResult::Clean(merged),
        Err(description) => MergeResult::Conflict(description),
    }
}

/// `merge_text`, except that a region both sides changed differently
/// resolves to `local`'s lines instead of failing. Every region only one
/// side changed still merges as usual, so `other`'s changes elsewhere in
/// the file are kept.
///
/// Binary content is still a conflict: there is no line-level region to
/// prefer, and returning `local` whole would discard all of `other`.
pub fn merge_text_prefer_local(base: &[u8], local: &[u8], other: &[u8]) -> MergeResult {
    if base == local && base == other {
        return MergeResult::Clean(base.to_vec());
    }
    if base == local {
        return MergeResult::Clean(other.to_vec());
    }
    if base == other {
        return MergeResult::Clean(local.to_vec());
    }
    if utils::is_binary(base) || utils::is_binary(local) || utils::is_binary(other) {
        return MergeResult::Conflict("binary file changed on both sides".to_string());
    }
    match merge3::merge3_with(base, local, other, merge3::ConflictPolicy::PreferA) {
        Ok(merged) => MergeResult::Clean(merged),
        Err(description) => MergeResult::Conflict(description),
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_merge_text_identical() {
        let content = b"hello\nworld\n";
        match merge_text(content, content, content) {
            MergeResult::Clean(result) => assert_eq!(result, content),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_only_local_changed() {
        let base = b"hello\nworld\n";
        let local = b"hello\nrust\n";
        match merge_text(base, local, base) {
            MergeResult::Clean(result) => assert_eq!(result, local),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_only_other_changed() {
        let base = b"hello\nworld\n";
        let other = b"hello\nrust\n";
        match merge_text(base, base, other) {
            MergeResult::Clean(result) => assert_eq!(result, other),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_clean_merge() {
        let base = b"line1\nline2\nline3\nline4\nline5\n";
        let local = b"modified1\nline2\nline3\nline4\nline5\n";
        let other = b"line1\nline2\nline3\nline4\nmodified5\n";
        match merge_text(base, local, other) {
            MergeResult::Clean(result) => {
                assert_eq!(result, b"modified1\nline2\nline3\nline4\nmodified5\n")
            }
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_conflict() {
        let base = b"line1\nline2\nline3\n";
        let local = b"line1\nmodified_a\nline3\n";
        let other = b"line1\nmodified_b\nline3\n";
        match merge_text(base, local, other) {
            MergeResult::Clean(_) => panic!("expected conflict"),
            MergeResult::Conflict(desc) => assert!(desc.contains("conflict")),
        }
    }

    #[mononoke::test]
    fn test_merge_text_binary_conflict() {
        let base = b"hello\0world";
        let local = b"hello\0rust";
        let other = b"hello\0python";
        match merge_text(base, local, other) {
            MergeResult::Clean(_) => panic!("expected conflict"),
            MergeResult::Conflict(desc) => assert!(desc.contains("binary")),
        }
    }

    #[mononoke::test]
    fn test_merge_text_binary_identical() {
        let content = b"binary\0content";
        match merge_text(content, content, content) {
            MergeResult::Clean(result) => assert_eq!(result, content),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_binary_one_side_changed() {
        let base = b"binary\0content";
        let local = b"binary\0modified";
        // When only one side changes, the fast path returns that side
        // without checking binary status (since there's no conflict).
        match merge_text(base, local, base) {
            MergeResult::Clean(result) => assert_eq!(result, local),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_empty_files() {
        match merge_text(b"", b"", b"") {
            MergeResult::Clean(result) => assert!(result.is_empty()),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_both_changed_identically() {
        let base = b"line1\nline2\nline3\n";
        let changed = b"line1\nmodified\nline3\n";
        match merge_text(base, changed, changed) {
            MergeResult::Clean(result) => assert_eq!(result, changed),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_prefer_local_resolves_overlap_keeps_other_elsewhere() {
        // Multi-line objects, as the real manifests are: a matching line
        // separates the two changed entries, so they are distinct regions.
        let base = b"{\n  \"A\": {\n    \"BUILD\": 1\n  },\n  \"B\": {\n    \"BUILD\": 1\n  }\n}\n";
        let local =
            b"{\n  \"A\": {\n    \"BUILD\": 1\n  },\n  \"B\": {\n    \"BUILD\": 5\n  }\n}\n";
        let other =
            b"{\n  \"A\": {\n    \"BUILD\": 2\n  },\n  \"B\": {\n    \"BUILD\": 7\n  }\n}\n";
        assert!(matches!(
            merge_text(base, local, other),
            MergeResult::Conflict(_)
        ));
        match merge_text_prefer_local(base, local, other) {
            // B (both changed) is local's; A (only other changed) is other's.
            MergeResult::Clean(result) => assert_eq!(
                result,
                b"{\n  \"A\": {\n    \"BUILD\": 2\n  },\n  \"B\": {\n    \"BUILD\": 5\n  }\n}\n"
            ),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_prefer_local_manifest_shape_keeps_other_entries() {
        // The AOSP prebuilt manifest shape: one multi-line object per
        // artifact, `},` and the next key between any two entries. Two
        // artifacts changed on different sides are separate regions even
        // when adjacent, so the pushed side's stale copy of the other
        // artifact is never taken. Server bumps A and B; client bumps B only.
        fn manifest(a: u32, b: u32, c: u32) -> Vec<u8> {
            format!(
                "{{\n    \"A\": {{\n        \"BUILD\": {a},\n        \"HASH\": \"a{a}\"\n    }},\n    \"B\": {{\n        \"BUILD\": {b},\n        \"HASH\": \"b{b}\"\n    }},\n    \"C\": {{\n        \"BUILD\": {c},\n        \"HASH\": \"c{c}\"\n    }}\n}}\n"
            )
            .into_bytes()
        }
        let base = manifest(1, 1, 1);
        let local = manifest(1, 5, 1);
        let other = manifest(2, 7, 1);
        assert!(matches!(
            merge_text(&base, &local, &other),
            MergeResult::Conflict(_)
        ));
        match merge_text_prefer_local(&base, &local, &other) {
            // A is other's (only other changed it), B is local's (both did).
            MergeResult::Clean(result) => assert_eq!(result, manifest(2, 5, 1)),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_prefer_local_adjacent_lines_coalesce() {
        // With no matching line between them, a line only `other` changed
        // and a line both changed form ONE conflict region, and PreferA
        // emits local's lines for all of it: other's `A=2` is lost. This is
        // the same sharp edge `git merge -X ours` has; callers must only
        // apply it to files whose entries are separated by unchanged lines.
        let base = b"A=1\nB=1\n";
        let local = b"A=1\nB=5\n";
        let other = b"A=2\nB=7\n";
        match merge_text_prefer_local(base, local, other) {
            MergeResult::Clean(result) => assert_eq!(result, b"A=1\nB=5\n"),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_prefer_local_binary_still_conflicts() {
        let base = b"a\x00b";
        match merge_text_prefer_local(base, b"a\x00c", b"a\x00d") {
            MergeResult::Conflict(desc) => assert!(desc.contains("binary")),
            MergeResult::Clean(_) => panic!("binary must not resolve"),
        }
    }

    #[mononoke::test]
    fn test_merge_text_prefer_local_one_side_changed_is_plain() {
        let base = b"1\n2\n";
        let other = b"1\n3\n";
        match merge_text_prefer_local(base, base, other) {
            MergeResult::Clean(result) => assert_eq!(result, other),
            MergeResult::Conflict(e) => panic!("unexpected conflict: {e}"),
        }
    }
}
