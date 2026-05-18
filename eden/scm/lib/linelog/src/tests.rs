/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use im::Vector as ImVec;
use rand_chacha::ChaChaRng;
use rand_core::Rng as _;
use rand_core::SeedableRng as _;

use crate::LineLog;

#[test]
fn test_empty() {
    let log = LineLog::default();
    assert_eq!(log.max_rev(), 0);
    assert_eq!(log.checkout_text(0), "");
    assert_eq!(log.checkout_text(1), "");
}

#[test]
fn test_edit_single() {
    let log = LineLog::default();
    let log = log.edit_chunk(0, 0, 0, 1, lines("c\nd\ne\n"));
    assert_eq!(log.checkout_text(0), "");
    assert_eq!(log.checkout_text(1), "c\nd\ne\n");
    assert_eq!(log.show(1), ["1:c", "1:d", "1:e", "0:"]);
}

#[test]
fn test_edit_rev0() {
    let log = LineLog::default();
    let log = log.edit_chunk(0, 0, 0, 0, lines("c\n"));
    assert_eq!(log.checkout_text(0), "c\n");
    let log = log.edit_chunk(0, 1, 1, 1, lines("d\n"));
    assert_eq!(log.checkout_text(0), "c\n");
    assert_eq!(log.checkout_text(1), "c\nd\n");
    assert_eq!(log.show(1), ["0:c", "1:d", "0:"]);
    // Edit an old version.
    let log = log.edit_chunk(0, 0, 0, 0, lines("b\n"));
    assert_eq!(log.checkout_text(1), "b\nc\nd\n");
    assert_eq!(log.show(1), ["0:b", "0:c", "1:d", "0:"]);
    // Try deletion.
    let log = log.edit_chunk(1, 1, 3, 2, lines("k\n"));
    assert_eq!(log.show_range(0, 2), ["0:b", "2:k", "-0:c", "-1:d", "-0:"]);
}

#[test]
fn test_random_cases() {
    fn generate_cases(
        end_rev: usize,
    ) -> impl Iterator<
        Item = (
            ImVec<String>,
            usize,
            usize,
            usize,
            usize,
            usize,
            Vec<String>,
        ),
    > {
        let mut rng = ChaChaRng::seed_from_u64(0);
        let max_delta_a = 10;
        let max_delta_b = 10;
        let max_b1 = 0xffff;
        let mut lines = ImVec::new();

        let mut rng_range = move |min: usize, max: usize| -> usize {
            let v = rng.next_u32() as usize;
            min + (v % (max + 1 - min))
        };

        (0..=end_rev).map(move |rev| {
            let n = lines.len();
            let a1 = rng_range(0, n);
            let a2 = rng_range(a1, n.min(a1 + max_delta_a));
            let b1 = rng_range(0, max_b1);
            let b2 = rng_range(b1, b1 + max_delta_b);
            let b_lines: Vec<String> = (b1..b2)
                .map(|b_idx| format!("{}:{}\n", rev, b_idx))
                .collect();

            let mut new_lines = lines.take(a1);
            new_lines.extend(b_lines.clone());
            new_lines.extend(lines.clone().slice(a2..));
            lines = new_lines;

            (lines.clone(), rev, a1, a2, b1, b2, b_lines)
        })
    }

    for (end_rev, a_rev_offset, b_rev_offset) in [(1000, 0, 0), (20, 0, 2), (20, 2, 0)] {
        let cases: Vec<_> = generate_cases(end_rev).collect();
        let mut log = LineLog::default();

        let mut line_count = 1;
        for (_lines, b_rev, a1, a2, b1, b2, b_lines) in &cases {
            let a_rev = log.max_rev();
            log = log.edit_chunk(
                a_rev + a_rev_offset,
                *a1,
                *a2,
                *b_rev + b_rev_offset,
                b_lines.clone(),
            );
            line_count += *b2 - *b1;
            line_count -= *a2 - *a1;
            assert_eq!(log.checkout_lines(*b_rev + b_rev_offset).len(), line_count);
        }

        for (lines, b_rev, _a1, _a2, _b1, _b2, _b_lines) in cases {
            let text = lines.into_iter().collect::<Vec<String>>().concat();
            assert_eq!(log.checkout_text(b_rev + b_rev_offset), text);
        }
    }
}

#[test]
fn test_describe_instructions() {
    let log = log_from_texts(&["a\n".into(), "b\n".into()]);
    // The instructions are internal details. For example, an
    // optimization pass might remove unconditional jumps.
    // Shall the output change, just update the test here.
    assert_eq!(
        log.describe_instructions(),
        vec![
            "0: J 1",
            "1: JL 1 3",
            "2: J 4",
            "3: END",
            "4: JL 2 6",
            "5: LINE 2 \"b\"",
            "6: JGE 2 3",
            "7: LINE 1 \"a\"",
            "8: J 3",
        ]
    );
}

#[test]
fn test_describe_ins_del_stacks_interleaved() {
    // First 3 revs are from https://sapling-scm.com/docs/internals/linelog
    let log = log_from_texts(
        &["a\nb\nc\n", "a\nb\n1\n2\nc\n", "a\n2\nc\n", "c\n", ""]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        log.describe_ins_del_stacks(),
        vec![
            "╭────Insert (rev 1)         ",
            "│    Delete (rev 4)    ────╮",
            "│    Line:  a              │",
            "│    Delete (rev 3)    ───╮│",
            "│    Line:  b             ││",
            "│╭───Insert (rev 2)       ││",
            "││   Line:  1             ││",
            "││                     ───╯│",
            "││   Line:  2              │",
            "│╰───                      │",
            "│                      ────╯",
            "│    Delete (rev 5)    ────╮",
            "│    Line:  c              │",
            "│                      ────╯",
            "╰────                       ",
        ]
    );
}

#[test]
fn test_describe_ins_del_stacks_not_nested() {
    // Insertions at the beginning and end are not nested.
    let log = log_from_texts(
        &["b\n", "a\nb\n", "a\nb\nc\n"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        log.describe_ins_del_stacks(),
        vec![
            "╭───Insert (rev 2)       ",
            "│   Line:  a             ",
            "╰───                     ",
            "╭───Insert (rev 1)       ",
            "│   Line:  b             ",
            "╰───                     ",
            "╭───Insert (rev 3)       ",
            "│   Line:  c             ",
            "╰───                     ",
        ]
    );
}

#[test]
fn test_describe_ins_del_stacks_between_old_new() {
    // Insertion between old new revs is not nested.
    let log = log_from_texts(
        &["a\n", "a\nc\n", "a\nb\nc\n"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        log.describe_ins_del_stacks(),
        vec![
            "╭───Insert (rev 1)       ",
            "│   Line:  a             ",
            "╰───                     ",
            "╭───Insert (rev 3)       ",
            "│   Line:  b             ",
            "╰───                     ",
            "╭───Insert (rev 2)       ",
            "│   Line:  c             ",
            "╰───                     ",
        ]
    );
}

#[test]
fn test_describe_ins_del_stacks_between_new_old() {
    // Insertion between new old revs is nested.
    let log = log_from_texts(
        &["c\n", "a\nc\n", "a\nb\nc\n"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        log.describe_ins_del_stacks(),
        vec![
            "╭────Insert (rev 2)       ",
            "│    Line:  a             ",
            "╰────                     ",
            "╭────Insert (rev 1)       ",
            "│╭───Insert (rev 3)       ",
            "││   Line:  b             ",
            "│╰───                     ",
            "│    Line:  c             ",
            "╰────                     ",
        ]
    );
}

#[test]
fn test_remap_revs() {
    let log = log_from_texts(&["b\n".into(), "b\nc\n".into(), "a\nb\nc\n".into()]);
    assert_eq!(log.checkout_text(1), "b\n");
    assert_eq!(log.checkout_text(2), "b\nc\n");
    assert_eq!(log.checkout_text(3), "a\nb\nc\n");

    // Swap rev 2 and 3.
    let swapped = log.clone().remap_revs(&|r| match r {
        2 => 3,
        3 => 2,
        other => other,
    });
    assert_eq!(swapped.max_rev(), 3);
    assert_eq!(swapped.checkout_text(3), "a\nb\nc\n");

    // Updates max_rev up.
    let mapped =
        log_from_texts(&["a\n".into(), "b\n".into()]).remap_revs(&|r| if r == 1 { 10 } else { r });
    assert_eq!(mapped.max_rev(), 10);

    // Updates max_rev down.
    let mapped =
        log_from_texts(&["a\n".into(), "b\n".into()]).remap_revs(&|r| if r == 2 { 1 } else { r });
    assert_eq!(mapped.max_rev(), 1);

    // Merge changes.
    let merged = log.clone().remap_revs(&|r| if r == 2 { 1 } else { r });
    assert_eq!(merged.checkout_text(1), "b\nc\n");
    assert_eq!(merged.checkout_text(3), "a\nb\nc\n");

    // Can insert changes by remapping to make room, then recording at the gap.
    let inserted = log_from_texts(&["b\n".into(), "b\nc\n".into()])
        .remap_revs(&|r| if r == 2 { 3 } else { r });
    let inserted = record_text(inserted, "a\nb\n", Some(2));
    assert_eq!(inserted.checkout_text(3), "a\nb\nc\n");

    // Does not check dependencies or conflicts.
    let log = log_from_texts(&["a\nc\n".into(), "a\nb\nc\n".into()]);
    let bad_swap = log.remap_revs(&|r| match r {
        1 => 2,
        2 => 1,
        other => other,
    });
    assert_eq!(bad_swap.checkout_text(1), "");
    assert_eq!(bad_swap.checkout_text(2), "a\nb\nc\n");
}

#[test]
fn test_remap_revs_reorder_insertions() {
    let log = log_from_texts(&["a\n".into(), "a\nb\n".into(), "a\nb\nc\n".into()]);

    let dep_map = log.calculate_dep_map();
    for rev in 1..=3 {
        assert_eq!(
            dep_map.get(&rev).map(|s| s.iter().collect::<Vec<_>>()),
            Some(vec![0]),
            "rev={rev}"
        );
    }

    let swapped = log.remap_revs(&|r| match r {
        2 => 3,
        3 => 2,
        other => other,
    });
    assert_eq!(swapped.checkout_text(3), "a\nb\nc\n");
}

/// Port of D52514621: test reordering for all insertion permutations.
///
/// If you append 2 functions in 2 commits, like:
///
///   Public    /* Previous code */
///   Commit 1 +
///   Commit 1 +function x() {
///   Commit 1 +  ...
///   Commit 1 +}
///   Commit 2 +
///   Commit 2 +function y() {
///   Commit 2 +  ...
///   Commit 2 +}
///
/// Then you can swap the 2 commits, but not swap back.
///
/// Tests cover all permutations of inserting 3 items, verifying
/// independence (dep only on rev 0) and correct content after
/// swapping rev 2 and 3.
///
/// Note the tests are kind of "strong" for pure insertions but it
/// still does not cover deletions yet.
#[test]
fn test_reorder_insertion_permutations() {
    let abc = ["a\n", "b\n", "c\n"];

    // All 6 permutations of which rev adds which line.
    let permutations: &[&[usize]] = &[
        &[1, 2, 3],
        &[1, 3, 2],
        &[2, 1, 3],
        &[2, 3, 1],
        &[3, 1, 2],
        &[3, 2, 1],
    ];

    for order in permutations {
        // FIXME: [2,3,1] order produces suboptimal dep: rev 3 depends on
        // rev 1 instead of rev 0. The block nesting causes a false dependency.
        let expected_dep_override = if *order == [2, 3, 1] {
            Some(vec![(1, vec![0]), (2, vec![0]), (3, vec![1])])
        } else {
            None
        };

        test_reorder_insertions(&abc, order, expected_dep_override);
    }
}

/// Swap revs 2 and 3 from a linelog built by inserting `lines` in the given
/// `line_added_order`. All lines are pure insertions by different revs.
///
/// For example, when lines = ["a\n", "b\n", "c\n"], line_added_order = [1, 3, 2]:
///   rev 1 adds "a\n", rev 2 adds "c\n", rev 3 adds "b\n".
///   texts: rev1 = "a\n", rev2 = "a\nc\n", rev3 = "a\nb\nc\n"
///
/// Verifies that (1) all revs depend only on rev 0 (independent),
/// and (2) after swapping rev 2 and 3, checkout produces correct content.
fn test_reorder_insertions(
    lines: &[&str],
    line_added_order: &[usize],
    expected_dep_override: Option<Vec<(usize, Vec<usize>)>>,
) {
    let n = lines.len();
    assert_eq!(n, line_added_order.len());
    let revs: Vec<usize> = (1..=n).collect();

    let texts = build_texts(lines, line_added_order, &revs);
    let log = log_from_texts(&texts);

    // Verify dep map.
    let dep_map = log.calculate_dep_map();
    let deps: Vec<(usize, Vec<usize>)> = dep_map
        .iter()
        .map(|(&rev, set)| (rev, set.iter().collect()))
        .collect();
    let expected =
        expected_dep_override.unwrap_or_else(|| revs.iter().map(|&r| (r, vec![0])).collect());
    assert_eq!(deps, expected, "order={line_added_order:?}");

    // Swap rev 2 and 3.
    let swap = |r: usize| match r {
        2 => 3,
        3 => 2,
        other => other,
    };
    let swapped = log.remap_revs(&swap);

    // Expected texts after swap.
    let swapped_revs: Vec<usize> = revs.iter().map(|&r| swap(r)).collect();
    let expected_texts = build_texts(lines, line_added_order, &swapped_revs);
    for &rev in &revs {
        assert_eq!(
            swapped.checkout_text(rev),
            expected_texts[rev - 1],
            "order={line_added_order:?}, rev={rev}"
        );
    }
}

/// Build text for each rev by accumulating lines in `rev_order`.
/// `line_added_order[i]` says which rev adds `lines[i]`.
/// Result[j] is the text at rev `rev_order[0..=j]` (lines whose adding rev
/// is in the accumulated set, preserving original line order).
fn build_texts(lines: &[&str], line_added_order: &[usize], rev_order: &[usize]) -> Vec<String> {
    use std::collections::HashSet;
    let mut rev_set = HashSet::new();
    rev_order
        .iter()
        .map(|&rev| {
            rev_set.insert(rev);
            lines
                .iter()
                .zip(line_added_order)
                .filter(|&(_, &order)| rev_set.contains(&order))
                .map(|(&line, _)| line)
                .collect()
        })
        .collect()
}

#[test]
fn test_truncate() {
    let texts: Vec<String> = ["a\nb\nc\n", "b\nc\nd\n", "b\nd\ne\n", "f\n"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let log = log_from_texts(&texts);

    for truncate_rev in 0..texts.len() {
        let truncated = log.clone().truncate(truncate_rev);
        assert_eq!(
            truncated.max_rev(),
            if truncate_rev == 0 {
                0
            } else {
                truncate_rev - 1
            }
        );
        for rev in 0..texts.len() {
            let text = truncated.checkout_text(rev);
            if rev < truncate_rev {
                let expected = if rev < 1 { "" } else { &texts[rev - 1] };
                assert_eq!(text, expected, "truncate={truncate_rev}, rev={rev}");
            } else {
                let expected = if truncate_rev <= 1 {
                    ""
                } else {
                    &texts[truncate_rev - 2]
                };
                assert_eq!(
                    text,
                    log.checkout_text(truncate_rev.saturating_sub(1)),
                    "truncate={truncate_rev}, rev={rev}"
                );
                assert_eq!(text, expected, "truncate={truncate_rev}, rev={rev}");
            }
        }
        let appended = record_text(truncated.clone(), "a\nc\ne\n", None);
        assert_eq!(appended.checkout_text(appended.max_rev()), "a\nc\ne\n");
        for rev in 0..truncate_rev {
            assert_eq!(appended.checkout_text(rev), log.checkout_text(rev));
        }
    }
}

#[test]
fn test_flatten() {
    // 3 revisions: rev1 "a b c", rev2 "b c d e", rev3 "a c d f".
    // Edits applied in reverse chunk order within each rev.
    let log = LineLog::default()
        .edit_chunk(0, 0, 0, 1, lines("a\nb\nc\n"))
        // rev 1 "a b c" -> rev 2 "b c d e": delete "a", insert "d e"
        .edit_chunk(1, 3, 3, 2, lines("d\ne\n"))
        .edit_chunk(1, 0, 1, 2, vec![])
        // rev 2 "b c d e" -> rev 3 "a c d f": replace "e"->"f", replace "b"->"a"
        .edit_chunk(2, 3, 4, 3, lines("f\n"))
        .edit_chunk(2, 0, 1, 3, lines("a\n"));

    assert_eq!(log.checkout_text(1), "a\nb\nc\n");
    assert_eq!(log.checkout_text(2), "b\nc\nd\ne\n");
    assert_eq!(log.checkout_text(3), "a\nc\nd\nf\n");

    let flat = log.flatten();
    let show: Vec<(&str, Vec<usize>)> = flat
        .iter()
        .map(|l| (l.data.trim_end(), l.revs.iter().collect()))
        .collect();
    assert_eq!(
        show,
        vec![
            ("a", vec![1]),
            ("a", vec![3]),
            ("b", vec![1, 2]),
            ("c", vec![1, 2, 3]),
            ("d", vec![2, 3]),
            ("f", vec![3]),
            ("e", vec![2]),
        ]
    );

    // Cross-check: filtering flatten lines by rev reconstructs the checkout.
    let text_list = ["a\nb\nc\n", "b\nc\nd\ne\n", "a\nc\nd\nf\n"];
    for rev in 1..=3 {
        let text: String = flat
            .iter()
            .filter(|l| l.revs.contains(rev))
            .map(|l| l.data.as_str())
            .collect();
        assert_eq!(text, text_list[rev - 1]);
    }
}

#[test]
fn test_calculate_dep_map() {
    let deps = |text_list: &[&str]| -> Vec<(usize, Vec<usize>)> {
        let texts: Vec<String> = text_list
            .iter()
            .map(|t| t.chars().map(|c| format!("{c}\n")).collect::<String>())
            .collect();
        let log = log_from_texts(&texts);
        let dep_map = log.calculate_dep_map();
        dep_map
            .into_iter()
            .map(|(rev, set)| (rev, set.iter().collect()))
            .collect()
    };

    assert_eq!(deps(&[]), Vec::<(usize, Vec<usize>)>::new());

    // Insertions.
    assert_eq!(deps(&["a"]), vec![(1, vec![0])]);
    assert_eq!(deps(&["a", "b"]), vec![(1, vec![0]), (2, vec![1])]);
    assert_eq!(deps(&["a", "ab"]), vec![(1, vec![0]), (2, vec![0])]);
    assert_eq!(deps(&["b", "ab"]), vec![(1, vec![0]), (2, vec![0])]);
    assert_eq!(
        deps(&["ad", "abd", "abcd"]),
        vec![(1, vec![0]), (2, vec![1]), (3, vec![1])]
    );
    assert_eq!(
        deps(&["ad", "acd", "abcd"]),
        vec![(1, vec![0]), (2, vec![1]), (3, vec![1])]
    );

    // Deletions.
    assert_eq!(
        deps(&["abcd", "abd", "ad", "a"]),
        vec![(1, vec![0]), (2, vec![1]), (3, vec![1]), (4, vec![1])]
    );
    assert_eq!(
        deps(&["abcd", "acd", "ad", "d"]),
        vec![(1, vec![0]), (2, vec![1]), (3, vec![1]), (4, vec![1])]
    );

    // Multi-rev insertion, then delete.
    assert_eq!(deps(&["abc", "abcdef", ""]).last(), Some(&(3, vec![1, 2])));
    assert_eq!(
        deps(&["abc", "abcdef", "af"]).last(),
        Some(&(3, vec![1, 2]))
    );
    assert_eq!(
        deps(&["abc", "abcdef", "cd"]).last(),
        Some(&(3, vec![1, 2]))
    );

    // Complex 9-rev scenario.
    let text_list = [
        "abc", "abcd", "zabcd", "zad", "ad", "adef", "ade", "ad1e", "xyz",
    ];
    assert_eq!(
        deps(&text_list),
        vec![
            (1, vec![0]),
            (2, vec![0]),
            (3, vec![0]),
            (4, vec![1]),             // deletes "bc" added by rev 1
            (5, vec![3]),             // deletes "z" added by rev 3
            (6, vec![0]),             // appends after "d", considered independent
            (7, vec![6]),             // deletes "f" added by rev 6
            (8, vec![0]),             // inserts "1" between "d" and "e", independent
            (9, vec![1, 2, 4, 6, 8]), // replaces all
        ]
    );
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(|s| format!("{}\n", s)).collect()
}

/// Build a LineLog by appending texts as successive revisions.
fn log_from_texts(texts: &[String]) -> LineLog {
    texts
        .iter()
        .fold(LineLog::default(), |log, text| record_text(log, text, None))
}

/// Record text at a specific rev, or append as a new rev if `rev` is None.
fn record_text(mut log: LineLog, text: &str, rev: Option<usize>) -> LineLog {
    let (a_rev, b_rev) = match rev {
        Some(r) => (r, r),
        None => (log.max_rev(), log.max_rev() + 1),
    };
    let a_lines_info = log.checkout_lines(a_rev);
    let a_text: Vec<String> = a_lines_info
        .iter()
        .take(a_lines_info.len() - 1)
        .map(|l| l.data.as_ref().clone())
        .collect();
    let b_lines: Vec<String> = text.lines().map(|l| format!("{l}\n")).collect();

    let blocks = diff_lines(&a_text, &b_lines);
    for (a1, a2, b1, b2) in blocks.into_iter().rev() {
        log = log.edit_chunk(a_rev, a1, a2, b_rev, b_lines[b1..b2].to_vec());
    }
    if log.max_rev() < b_rev {
        let n = log.checkout_lines(a_rev).len();
        log = log.edit_chunk(a_rev, n - 1, n - 1, b_rev, vec![]);
    }
    log
}

/// Simple LCS-based diff returning edit blocks [(a1, a2, b1, b2), ...].
fn diff_lines(a: &[String], b: &[String]) -> Vec<(usize, usize, usize, usize)> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut blocks = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            i += 1;
            j += 1;
        } else {
            let (ai, bj) = (i, j);
            while i < n && (j >= m || dp[i][j] == dp[i + 1][j]) {
                i += 1;
            }
            while j < m && (i >= n || dp[i][j] == dp[i][j + 1]) {
                j += 1;
            }
            blocks.push((ai, i, bj, j));
        }
    }
    blocks
}

impl LineLog {
    fn show(&self, rev: usize) -> Vec<String> {
        self.checkout_lines(rev)
            .into_iter()
            .map(|l| format!("{}:{}", l.rev, l.data.trim_end()))
            .collect()
    }

    fn show_range(&self, start: usize, end: usize) -> Vec<String> {
        self.checkout_range_lines(start, end)
            .into_iter()
            .map(|l| {
                format!(
                    "{}{}:{}",
                    if l.deleted { "-" } else { "" },
                    l.rev,
                    l.data.trim_end()
                )
            })
            .collect()
    }
}
