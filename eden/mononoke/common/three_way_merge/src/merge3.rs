/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Core 3-way merge algorithm.
//!
//! Implements the "sync regions" algorithm used by Git and Sapling:
//! find regions where both sides match the base, then classify the
//! gaps between them.

use std::ops::Range;

use crate::utils::compare_range;
use crate::utils::split_lines;

/// A matching block from xdiff: base_lines[base_start..base_end] matches
/// other_lines[other_start..other_end].
pub(crate) struct MatchingBlock {
    base_start: usize,
    other_start: usize,
    length: usize,
}

/// A sync region where both A and B match the base.
#[derive(Debug, PartialEq, Eq)]
struct SyncRegion {
    base: Range<usize>,
    a: Range<usize>,
    b: Range<usize>,
}

/// A classified region from the 3-way merge.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeRegion {
    /// Neither side changed this region. Use base[range].
    Unchanged { range: Range<usize> },
    /// Only side A changed. Use a[range].
    A { range: Range<usize> },
    /// Only side B changed. Use b[range].
    B { range: Range<usize> },
    /// Both sides changed identically. Use a[range].
    Same { range: Range<usize> },
    /// Both sides changed differently. Unresolvable conflict.
    Conflict {
        base: Range<usize>,
        a: Range<usize>,
        b: Range<usize>,
    },
}

/// Convert xdiff blocks output to MatchingBlock structs.
///
/// xdiff::blocks() returns (a1, a2, b1, b2) meaning a_lines[a1..a2] matches b_lines[b1..b2].
/// We convert to (base_start, other_start, length) for the sync region algorithm.
fn to_matching_blocks(raw_blocks: &[(u64, u64, u64, u64)]) -> Vec<MatchingBlock> {
    raw_blocks
        .iter()
        .map(|&(a1, a2, b1, _b2)| MatchingBlock {
            base_start: a1 as usize,
            other_start: b1 as usize,
            length: (a2 - a1) as usize,
        })
        .collect()
}

/// Find sync regions where both A and B match the base.
///
/// A sync region is a range in the base text that has matching content
/// in both A and B. These are found by intersecting the matching blocks
/// from base↔A and base↔B.
fn find_sync_regions(
    a_blocks: &[MatchingBlock],
    b_blocks: &[MatchingBlock],
    base_line_count: usize,
    a_line_count: usize,
    b_line_count: usize,
) -> Vec<SyncRegion> {
    let mut ia = 0;
    let mut ib = 0;
    let mut regions = Vec::new();

    while ia < a_blocks.len() && ib < b_blocks.len() {
        let a = &a_blocks[ia];
        let b = &b_blocks[ib];

        let a_base_end = a.base_start + a.length;
        let b_base_end = b.base_start + b.length;

        // Find intersection of the two base ranges
        let int_start = a.base_start.max(b.base_start);
        let int_end = a_base_end.min(b_base_end);

        if int_start < int_end {
            let int_len = int_end - int_start;

            // Map intersection back to A and B positions
            let a_sub = a.other_start + (int_start - a.base_start);
            let b_sub = b.other_start + (int_start - b.base_start);

            regions.push(SyncRegion {
                base: int_start..int_end,
                a: a_sub..a_sub + int_len,
                b: b_sub..b_sub + int_len,
            });
        }

        // Advance whichever one ends first in the base text
        if a_base_end < b_base_end {
            ia += 1;
        } else {
            ib += 1;
        }
    }

    // Sentinel: zero-length sync region at the end
    regions.push(SyncRegion {
        base: base_line_count..base_line_count,
        a: a_line_count..a_line_count,
        b: b_line_count..b_line_count,
    });

    regions
}

/// Classify all regions between sync points.
///
/// For each gap between sync regions, determine if:
/// - A changed and B didn't → take A
/// - B changed and A didn't → take B
/// - Both changed identically → take either (A)
/// - Both changed differently → conflict
pub(crate) fn merge_regions(
    base: &[&[u8]],
    a: &[&[u8]],
    b: &[&[u8]],
    a_blocks: &[MatchingBlock],
    b_blocks: &[MatchingBlock],
) -> Result<Vec<MergeRegion>, String> {
    let sync_regions = find_sync_regions(a_blocks, b_blocks, base.len(), a.len(), b.len());
    let mut regions = Vec::new();

    let mut iz: usize = 0;
    let mut ia: usize = 0;
    let mut ib: usize = 0;

    for sync in &sync_regions {
        let len_a = sync.a.start.checked_sub(ia).ok_or_else(|| {
            format!(
                "merge_regions: sync.a.start ({}) < ia ({})",
                sync.a.start, ia
            )
        })?;
        let len_b = sync.b.start.checked_sub(ib).ok_or_else(|| {
            format!(
                "merge_regions: sync.b.start ({}) < ib ({})",
                sync.b.start, ib
            )
        })?;

        if len_a > 0 || len_b > 0 {
            let equal_a = compare_range(a, ia..sync.a.start, base, iz..sync.base.start)?;
            let equal_b = compare_range(b, ib..sync.b.start, base, iz..sync.base.start)?;
            let same = compare_range(a, ia..sync.a.start, b, ib..sync.b.start)?;

            if same {
                regions.push(MergeRegion::Same {
                    range: ia..sync.a.start,
                });
            } else if equal_a && !equal_b {
                regions.push(MergeRegion::B {
                    range: ib..sync.b.start,
                });
            } else if equal_b && !equal_a {
                regions.push(MergeRegion::A {
                    range: ia..sync.a.start,
                });
            } else {
                regions.push(MergeRegion::Conflict {
                    base: iz..sync.base.start,
                    a: ia..sync.a.start,
                    b: ib..sync.b.start,
                });
            }

            ia = sync.a.start;
            ib = sync.b.start;
        }
        iz = sync.base.start;

        let match_len = sync.base.end - sync.base.start;
        if match_len > 0 {
            regions.push(MergeRegion::Unchanged {
                range: sync.base.start..sync.base.end,
            });
            iz = sync.base.end;
            ia = sync.a.end;
            ib = sync.b.end;
        }
    }

    Ok(regions)
}

/// Perform the full 3-way merge on line-split content.
///
/// Returns `Ok(merged_bytes)` on clean merge, or `Err(description)` on conflict.
pub(crate) fn merge3(base_bytes: &[u8], a_bytes: &[u8], b_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let base = split_lines(base_bytes);
    let a = split_lines(a_bytes);
    let b = split_lines(b_bytes);

    let raw_a_blocks = xdiff::blocks(base_bytes, a_bytes);
    let raw_b_blocks = xdiff::blocks(base_bytes, b_bytes);
    let a_blocks = to_matching_blocks(&raw_a_blocks);
    let b_blocks = to_matching_blocks(&raw_b_blocks);

    let regions = merge_regions(&base, &a, &b, &a_blocks, &b_blocks)?;

    let mut result = Vec::new();

    for region in &regions {
        match region {
            MergeRegion::Unchanged { range } => {
                for line in &base[range.clone()] {
                    result.extend_from_slice(line);
                }
            }
            MergeRegion::A { range } | MergeRegion::Same { range } => {
                for line in &a[range.clone()] {
                    result.extend_from_slice(line);
                }
            }
            MergeRegion::B { range } => {
                for line in &b[range.clone()] {
                    result.extend_from_slice(line);
                }
            }
            MergeRegion::Conflict {
                base: base_range,
                a: a_range,
                b: b_range,
            } => {
                let base_preview: Vec<String> = base[base_range.clone()]
                    .iter()
                    .take(3)
                    .map(|l| String::from_utf8_lossy(l).trim_end().to_string())
                    .collect();
                let preview = if base_preview.is_empty() {
                    "(insertion point)".to_string()
                } else if base_range.len() > 3 {
                    format!(
                        "{}... ({} lines)",
                        base_preview.join(", "),
                        base_range.len()
                    )
                } else {
                    base_preview.join(", ")
                };
                return Err(format!(
                    "conflict: both sides modified lines {}-{} of base ({}), \
                     local has {} lines, other has {} lines",
                    base_range.start + 1,
                    base_range.end,
                    preview,
                    a_range.len(),
                    b_range.len(),
                ));
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    // ==================== Clean merge tests ====================

    #[mononoke::test]
    fn test_no_changes() {
        let base = b"line1\nline2\nline3\n";
        let result = merge3(base, base, base);
        assert_eq!(result.unwrap(), base.to_vec());
    }

    #[mononoke::test]
    fn test_only_a_changed() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nmodified\nline3\n";
        let b = b"line1\nline2\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), a.to_vec());
    }

    #[mononoke::test]
    fn test_only_b_changed() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nline2\nline3\n";
        let b = b"line1\nmodified\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b.to_vec());
    }

    #[mononoke::test]
    fn test_both_changed_identically() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nmodified\nline3\n";
        let b = b"line1\nmodified\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), a.to_vec());
    }

    // ==================== Non-overlapping edits ====================

    #[mononoke::test]
    fn test_a_edits_top_b_edits_bottom() {
        let base = b"line1\nline2\nline3\nline4\nline5\n";
        let a = b"modified1\nline2\nline3\nline4\nline5\n";
        let b = b"line1\nline2\nline3\nline4\nmodified5\n";
        let result = merge3(base, a, b);
        assert_eq!(
            result.unwrap(),
            b"modified1\nline2\nline3\nline4\nmodified5\n".to_vec()
        );
    }

    #[mononoke::test]
    fn test_a_edits_lines_2_3_b_edits_lines_5_6() {
        let base = b"l1\nl2\nl3\nl4\nl5\nl6\nl7\n";
        let a = b"l1\na2\na3\nl4\nl5\nl6\nl7\n";
        let b = b"l1\nl2\nl3\nl4\nb5\nb6\nl7\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"l1\na2\na3\nl4\nb5\nb6\nl7\n".to_vec());
    }

    #[mononoke::test]
    fn test_interleaved_non_overlapping() {
        let base = b"l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\n";
        let a = b"l1\na2\na3\nl4\nl5\nl6\nl7\na8\na9\n";
        let b = b"l1\nl2\nl3\nl4\nb5\nb6\nl7\nl8\nl9\n";
        let result = merge3(base, a, b);
        assert_eq!(
            result.unwrap(),
            b"l1\na2\na3\nl4\nb5\nb6\nl7\na8\na9\n".to_vec()
        );
    }

    // ==================== Adjacent edits ====================

    #[mononoke::test]
    fn test_adjacent_edits_with_context() {
        // A changes line 3, B changes line 7 — separated by enough context
        // that xdiff treats them as separate hunks
        let base = b"l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\n";
        let a = b"l1\nl2\na3\nl4\nl5\nl6\nl7\nl8\nl9\n";
        let b = b"l1\nl2\nl3\nl4\nl5\nl6\nb7\nl8\nl9\n";
        let result = merge3(base, a, b);
        assert_eq!(
            result.unwrap(),
            b"l1\nl2\na3\nl4\nl5\nl6\nb7\nl8\nl9\n".to_vec()
        );
    }

    #[mononoke::test]
    fn test_truly_adjacent_edits_conflict() {
        // A changes line 3, B changes line 4 — truly adjacent lines are
        // treated as a single conflict region by xdiff (same as Git)
        let base = b"l1\nl2\nl3\nl4\nl5\n";
        let a = b"l1\nl2\na3\nl4\nl5\n";
        let b = b"l1\nl2\nl3\nb4\nl5\n";
        let result = merge3(base, a, b);
        assert!(
            result.is_err(),
            "truly adjacent edits produce a conflict, matching Git behavior"
        );
    }

    // ==================== Insertion tests ====================

    #[mononoke::test]
    fn test_a_adds_lines() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nnew_a\nline2\nline3\n";
        let b = b"line1\nline2\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"line1\nnew_a\nline2\nline3\n".to_vec());
    }

    #[mononoke::test]
    fn test_b_adds_lines() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nline2\nline3\n";
        let b = b"line1\nline2\nnew_b\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"line1\nline2\nnew_b\nline3\n".to_vec());
    }

    #[mononoke::test]
    fn test_both_add_same_at_same_point() {
        let base = b"line1\nline3\n";
        let a = b"line1\nnew_line\nline3\n";
        let b = b"line1\nnew_line\nline3\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"line1\nnew_line\nline3\n".to_vec());
    }

    #[mononoke::test]
    fn test_both_add_at_different_points() {
        let base = b"line1\nline2\nline3\nline4\n";
        let a = b"line1\nnew_a\nline2\nline3\nline4\n";
        let b = b"line1\nline2\nline3\nnew_b\nline4\n";
        let result = merge3(base, a, b);
        assert_eq!(
            result.unwrap(),
            b"line1\nnew_a\nline2\nline3\nnew_b\nline4\n".to_vec()
        );
    }

    // ==================== Deletion tests ====================

    #[mononoke::test]
    fn test_a_deletes_lines() {
        let base = b"line1\nline2\nline3\nline4\n";
        let a = b"line1\nline4\n";
        let b = b"line1\nline2\nline3\nline4\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"line1\nline4\n".to_vec());
    }

    #[mononoke::test]
    fn test_both_delete_same_lines() {
        let base = b"line1\nline2\nline3\nline4\n";
        let a = b"line1\nline4\n";
        let b = b"line1\nline4\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"line1\nline4\n".to_vec());
    }

    // ==================== Conflict tests ====================

    #[mononoke::test]
    fn test_same_line_different_edits() {
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nmodified_a\nline3\n";
        let b = b"line1\nmodified_b\nline3\n";
        let result = merge3(base, a, b);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("conflict"),
            "error should mention conflict: {err}"
        );
    }

    #[mononoke::test]
    fn test_overlapping_ranges() {
        // A edits lines 2-4, B edits lines 3-5
        let base = b"l1\nl2\nl3\nl4\nl5\nl6\n";
        let a = b"l1\na2\na3\na4\nl5\nl6\n";
        let b = b"l1\nl2\nb3\nb4\nb5\nl6\n";
        let result = merge3(base, a, b);
        assert!(result.is_err());
    }

    #[mononoke::test]
    fn test_delete_vs_modify() {
        // A deletes line2, B modifies it
        let base = b"line1\nline2\nline3\n";
        let a = b"line1\nline3\n";
        let b = b"line1\nmodified2\nline3\n";
        let result = merge3(base, a, b);
        assert!(result.is_err());
    }

    #[mononoke::test]
    fn test_both_add_different_at_same_point() {
        let base = b"line1\nline3\n";
        let a = b"line1\nnew_a\nline3\n";
        let b = b"line1\nnew_b\nline3\n";
        let result = merge3(base, a, b);
        assert!(result.is_err());
    }

    // ==================== Edge cases ====================

    #[mononoke::test]
    fn test_empty_base_both_add_same() {
        let base = b"";
        let a = b"new content\n";
        let b = b"new content\n";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"new content\n".to_vec());
    }

    #[mononoke::test]
    fn test_empty_base_both_add_different() {
        let base = b"";
        let a = b"content_a\n";
        let b = b"content_b\n";
        let result = merge3(base, a, b);
        assert!(result.is_err());
    }

    #[mononoke::test]
    fn test_all_empty() {
        let result = merge3(b"", b"", b"");
        assert_eq!(result.unwrap(), b"".to_vec());
    }

    #[mononoke::test]
    fn test_no_trailing_newline() {
        let base = b"line1\nline2";
        let a = b"modified1\nline2";
        let b = b"line1\nline2";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"modified1\nline2".to_vec());
    }

    #[mononoke::test]
    fn test_no_trailing_newline_both_sides() {
        let base = b"line1\nline2\nline3";
        let a = b"modified1\nline2\nline3";
        let b = b"line1\nline2\nmodified3";
        let result = merge3(base, a, b);
        assert_eq!(result.unwrap(), b"modified1\nline2\nmodified3".to_vec());
    }

    // ==================== Performance ====================

    #[mononoke::test]
    fn test_large_file_non_overlapping() {
        let mut base_lines = Vec::new();
        for i in 0..1000 {
            base_lines.push(format!("line {i}\n"));
        }
        let base: Vec<u8> = base_lines.iter().flat_map(|s| s.bytes()).collect();

        // A modifies lines near the start
        let mut a_lines = base_lines.clone();
        a_lines[5] = "modified_a_5\n".to_string();
        a_lines[6] = "modified_a_6\n".to_string();
        let a: Vec<u8> = a_lines.iter().flat_map(|s| s.bytes()).collect();

        // B modifies lines near the end
        let mut b_lines = base_lines.clone();
        b_lines[995] = "modified_b_995\n".to_string();
        b_lines[996] = "modified_b_996\n".to_string();
        let b: Vec<u8> = b_lines.iter().flat_map(|s| s.bytes()).collect();

        let result = merge3(&base, &a, &b);
        let merged = result.unwrap();

        // Verify the merge has both changes
        let merged_str = String::from_utf8(merged).unwrap();
        assert!(merged_str.contains("modified_a_5"));
        assert!(merged_str.contains("modified_a_6"));
        assert!(merged_str.contains("modified_b_995"));
        assert!(merged_str.contains("modified_b_996"));
        // Verify unchanged lines are preserved
        assert!(merged_str.contains("line 500\n"));
    }

    // ==================== Sync region internals ====================

    #[mononoke::test]
    fn test_find_sync_regions_identical() {
        let base = b"line1\nline2\nline3\n";
        let raw_a = xdiff::blocks(base, base);
        let raw_b = xdiff::blocks(base, base);
        let a_blocks = to_matching_blocks(&raw_a);
        let b_blocks = to_matching_blocks(&raw_b);
        let regions = find_sync_regions(&a_blocks, &b_blocks, 3, 3, 3);
        // Should have one sync region covering all lines + sentinel
        assert!(regions.len() >= 2);
        // First region should cover lines 0..3
        assert_eq!(regions[0].base.start, 0);
        assert_eq!(regions[0].base.end, 3);
    }
}
