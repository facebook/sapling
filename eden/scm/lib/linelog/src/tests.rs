/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use im::Vector as ImVec;
use rand_chacha::ChaChaRng;
use rand_core::Rng as _;
use rand_core::SeedableRng as _;
use smallvec::SmallVec;

use crate::AbstractLineLog;
use crate::CheckoutRev;
use crate::CheckoutRev::Single as R;
use crate::EditFlags;
use crate::EntryId as E;
use crate::LineLog;
use crate::SmallRevs;
use crate::linelog::Inst;
use crate::linelog::PerfStats;
use crate::linelog::Rev;
use crate::nanodag::NanoDag;

const E0: E = E(0);

#[test]
fn test_empty() {
    let log = LineLog::default();
    assert_eq!(log.max_rev(), 0);
    assert_eq!(log.entry_len(), 1);
    assert_eq!(log.checkout_text(E0, R(0)), "");
    assert_eq!(log.checkout_text(E0, R(1)), "");
}

#[test]
fn test_add_entry() {
    let log = LineLog::default();
    let (log, a) = log.add_entry();
    let (log, b) = log.add_entry();

    assert_eq!(a, E(1));
    assert_eq!(b, E(2));
    assert_eq!(log.entry_len(), 3);
    assert_eq!(log.entries, [0, 1, 2].into_iter().collect());
    assert!(log.code.iter().all(|inst| matches!(inst, Inst::END)));
}

#[test]
fn test_edit_multiple_entries() {
    let (log, e1) = LineLog::default().add_entry();
    let log = log
        .edit_chunk(E0, 0, 0, 0, 1, lines("a\n"), Default::default())
        .edit_chunk(e1, 0, 0, 0, 1, lines("b\n"), Default::default());

    assert_eq!(log.checkout_text(E0, R(1)), "a\n");
    assert_eq!(log.checkout_text(e1, R(1)), "b\n");
    assert_eq!(log.checkout_text(E0, R(0)), "");
    assert_eq!(log.checkout_text(e1, R(0)), "");
}

#[test]
fn test_rev_state() {
    struct RevState(String);
    fn get(log: &AbstractLineLog<String, RevState>, rev: Rev) -> Option<&str> {
        log.rev_state(rev).map(|state| state.as_ref().0.as_str())
    }
    fn set(log: AbstractLineLog<String, RevState>, rev: Rev) -> AbstractLineLog<String, RevState> {
        log.with_rev_state(rev, Some(Arc::new(RevState(format!("s{rev}")))))
    }
    fn state_list(
        log: &AbstractLineLog<String, RevState>,
        revs: impl Iterator<Item = Rev>,
    ) -> Vec<Option<&str>> {
        revs.map(|rev| get(log, rev)).collect()
    }

    let state = Arc::new(RevState("root".into()));
    let log = AbstractLineLog::<String, RevState>::default();
    assert!(log.rev_state(0).is_none());
    assert!(log.rev_state(1).is_none());

    let log = log.with_rev_state(0, Some(state.clone()));
    assert_eq!(get(&log, 0), Some("root"));
    assert!(Arc::ptr_eq(log.rev_state(0).unwrap(), &state));

    let log = log.with_rev_state(0, None);
    assert!(log.rev_state(0).is_none());

    let log = AbstractLineLog::<String, RevState>::default()
        .edit_chunk(E0, 0, 0, 0, 1, lines("a\n"), Default::default())
        .edit_chunk(E0, 1, 1, 1, 2, lines("b\n"), Default::default())
        .edit_chunk(E0, 2, 2, 2, 3, lines("c\n"), Default::default());
    let log = (0..=3).fold(log, set);

    // insert_shift keeps old states on their shifted revs and leaves the new rev empty.
    let inserted = log.clone().insert_shift(1);
    assert_eq!(
        state_list(&inserted, 0..=4),
        [Some("s0"), Some("s1"), None, Some("s2"), Some("s3")]
    );

    // topo_remap moves states according to the returned old-to-new mapping.
    let (remapped, old_to_new) = log
        .clone()
        .topo_remap(vec![
            SmallVec::new(),
            SmallVec::from_buf([0]),
            SmallVec::from_buf([3]),
            SmallVec::from_buf([1]),
        ])
        .unwrap();
    assert_eq!(old_to_new, vec![0, 1, 3, 2]);
    assert_eq!(
        state_list(&remapped, 0..=3),
        [Some("s0"), Some("s1"), Some("s3"), Some("s2")]
    );

    // truncate drops states for the truncated suffix.
    let truncated = log.truncate(2);
    assert_eq!(
        state_list(&truncated, 0..=2),
        [Some("s0"), Some("s1"), None]
    );
}

#[test]
fn test_edit_single() {
    let log = LineLog::default();
    let log = log.edit_chunk(E0, 0, 0, 0, 1, lines("c\nd\ne\n"), Default::default());
    assert_eq!(log.checkout_text(E0, R(0)), "");
    assert_eq!(log.checkout_text(E0, R(1)), "c\nd\ne\n");
    assert_eq!(log.show(1), ["1:c", "1:d", "1:e", "0:"]);
}

#[test]
fn test_edit_rev0() {
    let log = LineLog::default();
    let log = log.edit_chunk(E0, 0, 0, 0, 0, lines("c\n"), Default::default());
    assert_eq!(log.checkout_text(E0, R(0)), "c\n");
    let log = log.edit_chunk(E0, 0, 1, 1, 1, lines("d\n"), Default::default());
    assert_eq!(log.checkout_text(E0, R(0)), "c\n");
    assert_eq!(log.checkout_text(E0, R(1)), "c\nd\n");
    assert_eq!(log.show(1), ["0:c", "1:d", "0:"]);
    // Edit an old version.
    let log = log.edit_chunk(E0, 0, 0, 0, 0, lines("b\n"), Default::default());
    assert_eq!(log.checkout_text(E0, R(1)), "b\nc\nd\n");
    assert_eq!(log.show(1), ["0:b", "0:c", "1:d", "0:"]);
    // Try deletion.
    let log = log.edit_chunk(E0, 1, 1, 3, 2, lines("k\n"), Default::default());
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
                .map(|b_idx| {
                    if rng_range(0, 2) == 0 {
                        format!("{rev}:{b_idx}\n")
                    } else {
                        // Exercise block shifting more easily.
                        "\n".to_string()
                    }
                })
                .collect();

            let mut new_lines = lines.take(a1);
            new_lines.extend(b_lines.clone());
            new_lines.extend(lines.clone().slice(a2..));
            lines = new_lines;

            (lines.clone(), rev, a1, a2, b1, b2, b_lines)
        })
    }

    for (end_rev, initial_rev_offset, b_rev_offset) in [(1000, 0, 0), (20, 0, 2), (20, 2, 0)] {
        let mut cases: Vec<_> = generate_cases(end_rev).collect();
        let stats = Arc::new(PerfStats::default());
        let mut log = LineLog::default().with_perf_stats(Some(stats.clone()));

        if initial_rev_offset > 0 {
            log = log.edit_chunk(
                E0,
                0,
                0,
                0,
                initial_rev_offset,
                Vec::new(),
                Default::default(),
            )
        }

        let mut line_count = 1;
        for (_lines, b_rev, a1, a2, b1, b2, b_lines) in &mut cases {
            let a_rev = log.max_rev();
            *b_rev = *b_rev + b_rev_offset + initial_rev_offset;
            assert!(*b_rev >= a_rev);
            log = log.edit_chunk(
                E0,
                a_rev,
                *a1,
                *a2,
                *b_rev,
                b_lines.clone(),
                Default::default(),
            );
            line_count += *b2 - *b1;
            line_count -= *a2 - *a1;
            assert_eq!(log.checkout_lines(E0, R(*b_rev)).len(), line_count);
        }

        // execute prepares ancestor revsets once, then reuses the dag cache.
        assert_eq!(stats.dag_cache.load(Ordering::Acquire), 1);
        // All in "happy" cache_hit paths. "execute" called O(1) times.
        assert_eq!(stats.execute.load(Ordering::Acquire), 1);

        for (lines, b_rev, _a1, _a2, _b1, _b2, _b_lines) in cases {
            let text = lines.into_iter().collect::<Vec<String>>().concat();
            assert_eq!(log.checkout_text(E0, R(b_rev)), text);
        }
    }
}

#[test]
#[should_panic(expected = "must not be greater than max_rev")]
fn test_edit_chunk_rejects_future_a_rev() {
    let log = LineLog::default();
    let _ = log.edit_chunk(E0, 1, 0, 0, 1, lines("a\n"), Default::default());
}

#[test]
fn test_a_lines_cache_effectiveness() {
    let stats = Arc::new(PerfStats::default());
    let log = LineLog::default().with_perf_stats(Some(stats.clone()));

    let check = |label: &str, expected_hits: usize, expected_execs: usize| {
        let hits = stats.cache_hit.load(Ordering::Acquire);
        let execs = stats.execute.load(Ordering::Acquire);
        assert_eq!((hits, execs), (expected_hits, expected_execs), "{label}");
    };

    // Cold start: a_rev=0, b_rev=1. No cache yet, requires execute.
    let log = log.edit_chunk(E0, 0, 0, 0, 1, lines("a\nb\nc\n"), Default::default());
    check("after rev 1 insert", 0, 1);

    // a_rev=1, b_rev=1 (edit within same rev). Cache has (1, ...) from
    // above, so a_rev=1 hits.
    let log = log.edit_chunk(E0, 1, 1, 1, 1, lines("x\n"), Default::default());
    check("after rev 1 edit same rev", 1, 1);

    // a_rev=1, b_rev=2. Cache has (1, ...), a_rev=1 hits.
    let log = log.edit_chunk(E0, 1, 0, 1, 2, vec![], Default::default());
    check("after rev 2 delete", 2, 1);

    // a_rev=2, b_rev=3. Cache has (2, ...), a_rev=2 hits.
    let log = log.edit_chunk(E0, 2, 1, 1, 3, lines("d\n"), Default::default());
    check("after rev 3 insert", 3, 1);

    // Verify the content is correct despite heavy caching, and checkout hits cache too.
    assert_eq!(log.checkout_text(E0, R(3)), "x\nd\nb\nc\n");
    check("after checkout", 4, 1);

    // Verify the dag cache (for ancestors and descendants) only gets built O(1) times.
    assert_eq!(stats.dag_cache.load(Ordering::Acquire), 1);
}

#[test]
fn test_a_lines_cache_does_not_cache_invisible_edit_without_edge() {
    let stats = Arc::new(PerfStats::default());
    let log = LineLog::default().with_perf_stats(Some(stats.clone()));
    // Disabling ADD_EDGE is a power-user use case.
    let flags = EditFlags::default() - EditFlags::ADD_EDGE;

    let log = log
        .edit_chunk(E0, 0, 0, 0, 0, lines("a\nb\n"), flags)
        .edit_chunk(E0, 0, 1, 1, 1, lines("c\n"), flags);

    // rev 1's "c\n" is invisible:
    // During checkout(rev 1) (in LineLog::execute), the outer rev 0 block is
    // skipped (checked dag), so the rev 1 insertion inside rev 0 chunk is
    // skipped too, becomes invisible.
    let cache_hit_before = stats.cache_hit.load(Ordering::Acquire);
    assert_eq!(log.checkout_text(E0, R(1)), "");
    let cache_hit_after = stats.cache_hit.load(Ordering::Acquire);

    // No cache hit during checkout: edit_chunk cannot prepare the cache without
    // the parent edge.
    assert_eq!(cache_hit_before, cache_hit_after);

    // linelog dep dag, rev 1 depends on rev 0 (insert into rev 0 block)
    assert_eq!(log.dep_dag().to_string(), "0-1");
    // dag edges, rev 1 does not depend on rev 0
    assert_eq!(log.nanodag().to_string(), "{0,1}");
    // Note that dep_dag is usually a subset of dag. If dep_dag has edges that
    // dag does not have, it means the "invisible" problem can occur.
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
    // Insertion between new old revs is not nested, for easier reordering.
    let log = log_from_texts(
        &["c\n", "a\nc\n", "a\nb\nc\n"]
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
            "╭───Insert (rev 3)       ",
            "│   Line:  b             ",
            "╰───                     ",
            "╭───Insert (rev 1)       ",
            "│   Line:  c             ",
            "╰───                     ",
        ]
    );
}

#[test]
fn test_example_merge() {
    // Demonstrate how to do a merge, and the linelog internals of the merge.
    // 0-{1,2}-3. 1 and 2 both inserts and deletes something.
    let mut log = LineLog::default();
    log = record_text(log, "b\nc\nd\n", 0, 0);
    // Left side: delete "d", insert "a".
    log = record_text(log, "a\nb\nc\n", 0, 1);
    // Right side: delete "b", append "e".
    log = record_text(log, "c\nd\ne\n", 0, 2);
    // Create edges for the merge first, so merge has one rev (3) to be edited on.
    log = log.with_dag_edge(2, 3).with_dag_edge(1, 3);
    // The merge changes "c" (not modified by both sides) to "C".
    log = record_text(log, "a\nC\ne\n", 3, 3);
    // Note the linelog internals, the merge itself does not re-create the edits
    // made by either side.
    assert_eq!(
        log.describe_ins_del_stacks(),
        [
            "╭────Insert (rev 1)        ",
            "│    Line:  a              ",
            "╰────                      ",
            "╭────Insert (rev 0)        ",
            "│    Delete (rev 2)    ───╮",
            "│    Line:  b             │",
            "│                      ───╯",
            "│╭───Insert (rev 3)        ",
            "││   Line:  C              ",
            "│╰───                      ",
            "│    Delete (rev 3)    ───╮",
            "│    Line:  c             │",
            "│                      ───╯",
            "│    Delete (rev 1)    ───╮",
            "│    Line:  d             │",
            "╰────                     │",
            "                       ───╯",
            "╭────Insert (rev 2)        ",
            "│    Line:  e              ",
            "╰────                      "
        ]
    );

    // CheckoutRev differences.
    let revs = SmallRevs::from_range(1..=2);
    // CheckoutRev::Merge respects the deletion on each side.
    assert_eq!(
        log.checkout_text(E0, CheckoutRev::Merge(revs.clone())),
        r#"a
c
e
"#
    );
    // CheckoutRev::Range keeps the deleted lines, since they exist in the other side.
    assert_eq!(
        log.checkout_text(E0, CheckoutRev::Range(revs.clone())),
        r#"a
b
c
d
e
"#
    );

    // Flatten view
    let flat = log.flatten();
    let show: Vec<String> = flat
        .iter()
        .map(|l| format!("{} {:?}", l.data.trim_end(), l.revs))
        .collect();
    assert_eq!(
        show,
        [
            "a {1, 3}",
            "b {0, 1}",
            "C {3}",
            "c {0, 1, 2}",
            "d {0, 2}",
            "e {2, 3}"
        ],
    );
}

#[test]
fn test_remap_code_revs() {
    let log = log_from_texts(&["b\n".into(), "b\nc\n".into(), "a\nb\nc\n".into()]);
    assert_eq!(log.checkout_text(E0, R(1)), "b\n");
    assert_eq!(log.checkout_text(E0, R(2)), "b\nc\n");
    assert_eq!(log.checkout_text(E0, R(3)), "a\nb\nc\n");

    // Swap rev 2 and 3.
    let swapped = log.clone().remap_code_revs(&|r| match r {
        2 => 3,
        3 => 2,
        other => other,
    });
    assert_eq!(swapped.max_rev(), 3);
    assert_eq!(swapped.checkout_text(E0, R(3)), "a\nb\nc\n");

    // Updates max_rev up.
    let mapped = log_from_texts(&["a\n".into(), "b\n".into()])
        .remap_code_revs(&|r| if r == 1 { 10 } else { r });
    assert_eq!(mapped.max_rev(), 10);

    // Updates max_rev down.
    let mapped = log_from_texts(&["a\n".into(), "b\n".into()])
        .remap_code_revs(&|r| if r == 2 { 1 } else { r });
    assert_eq!(mapped.max_rev(), 1);

    // Merge changes.
    let merged = log.clone().remap_code_revs(&|r| if r == 2 { 1 } else { r });
    assert_eq!(merged.checkout_text(E0, R(1)), "b\nc\n");
    assert_eq!(merged.checkout_text(E0, R(3)), "a\nb\nc\n");

    // Can insert changes by shifting revs to make room, then recording at the gap.
    let log = log_from_texts(&["b\n".into(), "b\nc\n".into()]);
    assert_eq!(log.max_rev(), 2);
    let inserted = log.insert_shift(1);
    assert_eq!(inserted.max_rev(), 3);
    let inserted = record_text(inserted, "a\nb\n", 1, 2);
    assert_eq!(inserted.checkout_text(E0, R(3)), "a\nb\nc\n");

    // Raw code remapping does not update the dag or validate dependencies.
    let log = log_from_texts(&["a\nc\n".into(), "a\nb\nc\n".into()]);
    let bad_swap = log.remap_code_revs(&|r| match r {
        1 => 2,
        2 => 1,
        other => other,
    });
    assert_eq!(bad_swap.checkout_text(E0, R(1)), "");
    assert_eq!(bad_swap.checkout_text(E0, R(2)), "a\nb\nc\n");
}

#[test]
fn test_fold_revs() {
    let mut log = LineLog::default();
    log = record_text(log, "b\n", 0, 0);
    log = record_text(log, "b\nc\n", 0, 1);
    log = record_text(log, "b\nC\n", 1, 2);
    log = record_text(log, "d\nb\n", 0, 3);
    log = log.with_dag_edge(3, 4).with_dag_edge(2, 4);
    log = record_text(log, "d\nb\nC\n", 4, 4);

    assert_eq!(log.nanodag().to_string(), "0-{1-2,3}-4");
    assert_eq!(log.checkout_text(E0, R(1)), "b\nc\n");
    assert_eq!(log.checkout_text(E0, R(2)), "b\nC\n");
    assert_eq!(log.checkout_text(E0, R(3)), "d\nb\n");
    assert_eq!(log.checkout_text(E0, R(4)), "d\nb\nC\n");

    // Fold {1-2,3}, two branches into one rev.
    let folded = log.clone().fold(&SmallRevs::from_range(1..=3)).unwrap();

    assert_eq!(folded.nanodag().to_string(), "{0-1-4,2,3}");

    assert_eq!(
        folded.checkout_text(E0, R(1)),
        log.checkout_text(E0, CheckoutRev::Merge(SmallRevs::from_range(2..=3)))
    );
    assert_eq!(folded.checkout_text(E0, R(2)), "");
    assert_eq!(folded.checkout_text(E0, R(3)), "");
    assert_eq!(folded.checkout_text(E0, R(4)), log.checkout_text(E0, R(4)));
}

#[test]
fn test_topo_remap_revs() {
    fn parents(parents: &[&[Rev]]) -> Vec<SmallVec<[Rev; 1]>> {
        parents
            .iter()
            .map(|parents| parents.iter().copied().collect())
            .collect()
    }

    let log = log_from_texts(&["a\n".into(), "a\nb\n".into(), "a\nb\nc\n".into()]);
    assert_eq!(log.nanodag().to_string(), "0-1-2-3");

    // Reorder append-only revs: old 0-1-2-3, proposed old edges
    // 0-1-3-2. The returned log is renumbered back to 0-1-2-3, with
    // old rev 3 becoming new rev 2.
    let (remapped, old_to_new) = log
        .clone()
        .topo_remap(parents(&[&[], &[0], &[3], &[1]]))
        .expect("append-only revs can be reordered");
    assert_eq!(old_to_new, vec![0, 1, 3, 2]);
    assert_eq!(remapped.nanodag().to_string(), "0-1-2-3");
    assert_eq!(remapped.checkout_text(E0, R(1)), "a\n");
    assert_eq!(remapped.checkout_text(E0, R(2)), "a\nc\n");
    assert_eq!(remapped.checkout_text(E0, R(3)), "a\nb\nc\n");

    // Split 0-1-2-3 into 0-2 and 1-3. Since these append-only revs have no
    // textual dependencies on each other, dep_dag allows the split.
    let (split, old_to_new) = log
        .clone()
        .topo_remap(parents(&[&[], &[], &[0], &[1]]))
        .expect("independent revs can be split into disjoint chains");
    assert_eq!(old_to_new, vec![0, 1, 2, 3]);
    assert_eq!(split.nanodag().to_string(), "{0-2,1-3}");
    assert_eq!(split.checkout_text(E0, R(2)), "b\n");
    assert_eq!(split.checkout_text(E0, R(3)), "a\nc\n");

    // Join the split chains back into a linear history.
    let (joined, old_to_new) = split
        .topo_remap(parents(&[&[], &[0], &[1], &[2]]))
        .expect("split chains can be joined back");
    assert_eq!(old_to_new, vec![0, 1, 2, 3]);
    assert_eq!(joined.nanodag().to_string(), "0-1-2-3");
    assert_eq!(joined.checkout_text(E0, R(3)), "a\nb\nc\n");
}

#[test]
fn test_remap_code_revs_reorder_insertions() {
    let log = log_from_texts(&["a\n".into(), "a\nb\n".into(), "a\nb\nc\n".into()]);

    let dep_dag = log.dep_dag();
    // Those append-only changes are considered independent.
    // Not depending on `0` - `0` used to be the "root" that other appends
    // depend on. But with non-linear (dag) linelog, `0` is no longer special.
    for rev in 1..=3 {
        assert_eq!(dep_dag.parents(rev), [], "rev={rev}");
    }

    let swapped = log.remap_code_revs(&|r| match r {
        2 => 3,
        3 => 2,
        other => other,
    });
    assert_eq!(swapped.checkout_text(E0, R(3)), "a\nb\nc\n");
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
        test_reorder_insertions(&abc, order);
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
fn test_reorder_insertions(lines: &[&str], line_added_order: &[usize]) {
    let n = lines.len();
    assert_eq!(n, line_added_order.len());
    let revs: Vec<usize> = (1..=n).collect();

    let texts = build_texts(lines, line_added_order, &revs);
    let log = log_from_texts(&texts);

    // Verify dep dag.
    let deps = log.dep_dag();
    assert!(
        deps.iter().all(|(_rev, deps)| deps.is_empty()),
        "order={line_added_order:?}"
    );

    // Swap rev 2 and 3.
    let swap = |r: usize| match r {
        2 => 3,
        3 => 2,
        other => other,
    };
    let swapped = log.remap_code_revs(&swap);

    // Expected texts after swap.
    let swapped_revs: Vec<usize> = revs.iter().map(|&r| swap(r)).collect();
    let expected_texts = build_texts(lines, line_added_order, &swapped_revs);
    for &rev in &revs {
        assert_eq!(
            swapped.checkout_text(E0, R(rev)),
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
            let text = truncated.checkout_text(E0, R(rev));
            if rev < truncate_rev {
                let expected = if rev < 1 { "" } else { &texts[rev - 1] };
                assert_eq!(text, expected, "truncate={truncate_rev}, rev={rev}");
            } else {
                let expected = "";
                assert_eq!(
                    text,
                    truncated.checkout_text(E0, R(rev)),
                    "truncate={truncate_rev}, rev={rev}"
                );
                assert_eq!(text, expected, "truncate={truncate_rev}, rev={rev}");
            }
        }
        let appended = append_text(truncated.clone(), "a\nc\ne\n");
        assert_eq!(
            appended.checkout_text(E0, R(appended.max_rev())),
            "a\nc\ne\n"
        );
        for rev in 0..truncate_rev {
            assert_eq!(
                appended.checkout_text(E0, R(rev)),
                log.checkout_text(E0, R(rev))
            );
        }
    }
}

#[test]
fn test_non_linear_skipped_rev() {
    let flags = EditFlags::default();
    // rev 0 has content, rev 1 is skipped (not depend on rev 0), rev 2 depends on rev 1.
    let log = AbstractLineLog::<&str>::default()
        .edit_chunk(E0, 0, 0, 0, 0, vec!["a", "c"], flags)
        .edit_chunk(E0, 0, 1, 1, 2, vec!["b"], flags);
    assert_eq!(log.nanodag().to_string(), "{0-2,1}");
    assert_eq!(log.dep_dag().to_string(), "{0-2,1}");
    assert_eq!(log.checkout_text(E0, R(0)), "ac");
    assert_eq!(log.checkout_text(E0, R(1)), "");
    assert_eq!(log.checkout_text(E0, R(2)), "abc");
}

#[test]
fn test_non_linear_merged_rev() {
    let flags = EditFlags::default();
    // rev 0: a -> rev 1: b b    ------------> b
    // rev 0: c                  --> rev 3 --> x
    // rev 0: d ----> rev 2: e e ------------> e
    // rev 0: f
    let log = AbstractLineLog::<&str>::default()
        .with_dag_edge(3, 3)
        .edit_chunk(E0, 0, 0, 0, 0, vec!["a", "c", "d", "f"], flags)
        .edit_chunk(E0, 0, 0, 1, 1, vec!["b", "b"], flags)
        .edit_chunk(E0, 0, 2, 3, 2, vec!["e", "e"], flags)
        .with_dag_edge(2, 3)
        .with_dag_edge(1, 3);
    assert_eq!(log.nanodag().to_string(), "0-{1,2}-3");
    assert_eq!(log.dep_dag().to_string(), "{0-{1,2},3}");
    assert_eq!(log.checkout_text(E0, R(0)), "acdf"); // rev 0, orig content
    assert_eq!(log.checkout_text(E0, R(1)), "bbcdf"); // rev 1 replaced "a" with "bb"
    assert_eq!(log.checkout_text(E0, R(2)), "aceef"); // rev 2 replaced "d" with "ee", without rev 1 "bb"
    assert_eq!(log.checkout_text(E0, R(3)), "bbceef"); // rev 3 is a (unchanged) merge, with both "bb" and "ee"

    // changes on the default merge result
    let log = log.edit_chunk(E0, 3, 1, 4, 3, vec!["x"], flags);
    assert_eq!(log.checkout_text(E0, R(3)), "bxef"); // rev 3 replaced the middle "bce" with "x"
}

#[test]
fn test_flatten() {
    // 3 revisions: rev1 "a b c", rev2 "b c d e", rev3 "a c d f".
    // Edits applied in reverse chunk order within each rev.
    let log = LineLog::default()
        .edit_chunk(E0, 0, 0, 0, 1, lines("a\nb\nc\n"), Default::default())
        // rev 1 "a b c" -> rev 2 "b c d e": delete "a", insert "d e"
        .edit_chunk(E0, 1, 3, 3, 2, lines("d\ne\n"), Default::default())
        .edit_chunk(E0, 1, 0, 1, 2, vec![], Default::default())
        // rev 2 "b c d e" -> rev 3 "a c d f": replace "e"->"f", replace "b"->"a"
        .edit_chunk(E0, 2, 3, 4, 3, lines("f\n"), Default::default())
        .edit_chunk(E0, 2, 0, 1, 3, lines("a\n"), Default::default());

    assert_eq!(log.checkout_text(E0, R(1)), "a\nb\nc\n");
    assert_eq!(log.checkout_text(E0, R(2)), "b\nc\nd\ne\n");
    assert_eq!(log.checkout_text(E0, R(3)), "a\nc\nd\nf\n");

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
fn test_dep_dag() {
    let deps = |text_list: &[&str]| -> Arc<NanoDag> {
        let texts: Vec<String> = text_list
            .iter()
            .map(|t| t.chars().map(|c| format!("{c}\n")).collect::<String>())
            .collect();
        let log = log_from_texts(&texts);
        log.dep_dag().clone()
    };

    assert_eq!(deps(&[]).to_string(), "0");

    // rev 1 introduces initial content, it is dependent-free.
    assert_eq!(deps(&["a"]).to_string(), "{0,1}");
    // rev 2 "b" deletes "a" (rev 1) and adds "b", depends on rev 1.
    assert_eq!(deps(&["a", "b"]).to_string(), "{0,1-2}");
    // rev 2 appends "b", do not depend on rev 1 (free to reorder).
    assert_eq!(deps(&["a", "ab"]).to_string(), "{0,1,2}");
    // rev 2 inserts "b", do not depend on rev 1 (free to reorder).
    assert_eq!(deps(&["b", "ab"]).to_string(), "{0,1,2}");
    // rev 3 inserts "b" or "c", next to rev 2, in the middle of rev 1, only depends on rev 1.
    assert_eq!(deps(&["ad", "abd", "abcd"]).to_string(), "{0,1-{2,3}}");
    assert_eq!(deps(&["ad", "acd", "abcd"]).to_string(), "{0,1-{2,3}}");

    // rev 0 can still be depended on, if its content is not empty.
    {
        let log = record_text(
            record_text(LineLog::default(), "a\nc\n", 0, 0),
            "a\nb\nc\n",
            0,
            1,
        );
        assert_eq!(log.dep_dag().to_string(), "0-1");
    }

    // Deletions.
    // rev 2, 3, 4 each deletes one character from "abcd", rev 1.
    // rev 2, 3, 4 do not depend on each other, but all depend on rev 1.
    assert_eq!(
        deps(&["abcd", "abd", "ad", "a"]).to_string(),
        "{0,1-{2,3,4}}"
    );
    assert_eq!(
        deps(&["abcd", "acd", "ad", "d"]).to_string(),
        "{0,1-{2,3,4}}"
    );

    // Multi-rev insertion, then delete.
    // rev 3 deletes both parts of rev 1, and rev 2, so depends on both.
    assert_eq!(deps(&["abc", "abcdef", ""]).to_string(), "{0,1-3,2-3}",);
    assert_eq!(deps(&["abc", "abcdef", "af"]).to_string(), "{0,1-3,2-3}");
    assert_eq!(deps(&["abc", "abcdef", "cd"]).to_string(), "{0,1-3,2-3}");

    // Complex 9-rev scenario.
    let text_list = [
        "abc", "abcd", "zabcd", "zad", "ad", "adef", "ade", "ad1e", "xyz",
    ];
    assert_eq!(
        deps(&text_list).to_string(),
        // rev 2: appends "d", do not depend on sibling line rev 1
        // rev 3: inserts "z", do not depend on sibling line rev 1
        // rev 4: deletes "bc" added by rev 1
        // rev 5: deletes "z" added by rev 3
        // rev 6: appends "ef" after EOF "d", considered independent
        // rev 7: deletes "f" added by rev 6
        // rev 8: inserts "1" between "d" (rev 2) and "e" (rev 6), independent
        // rev 9: replace all, depends on [1, 2, 4, 6, 8]
        "{0,1-{4,}-9,2-9,3-5,6-{7,9},8-9}",
    );
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(|s| format!("{s}\n")).collect()
}

/// Build a LineLog by appending texts as successive revisions.
fn log_from_texts(texts: &[String]) -> LineLog {
    texts
        .iter()
        .fold(LineLog::default(), |log, text| append_text(log, text))
}

/// Append text as a new revision based on the current max revision.
fn append_text(log: LineLog, text: &str) -> LineLog {
    let a_rev = log.max_rev();
    record_text(log, text, a_rev, a_rev + 1)
}

/// Record text at `b_rev`, using `a_rev` as the base revision.
///
/// `a_rev == b_rev` is valid for editing a revision that already exists. Callers
/// that create a new revision should pass the actual parent as `a_rev`.
fn record_text(mut log: LineLog, text: &str, a_rev: usize, b_rev: usize) -> LineLog {
    let a_lines_info = log.checkout_lines(E0, R(a_rev));
    let a_text: Vec<String> = a_lines_info
        .iter()
        .take(a_lines_info.len() - 1)
        .map(|l| l.data.as_ref().clone())
        .collect();
    let b_lines: Vec<String> = text.lines().map(|l| format!("{l}\n")).collect();

    let blocks = diff_lines(&a_text, &b_lines);
    for (a1, a2, b1, b2) in blocks.into_iter().rev() {
        log = log.edit_chunk(
            E0,
            a_rev,
            a1,
            a2,
            b_rev,
            b_lines[b1..b2].to_vec(),
            Default::default(),
        );
    }
    if log.max_rev() < b_rev {
        let n = log.checkout_lines(E0, R(a_rev)).len();
        log = log.edit_chunk(E0, a_rev, n - 1, n - 1, b_rev, vec![], Default::default());
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

/// Test that block shifting avoids false dependencies when inserting
/// in the middle of an existing insertion block, at various offsets.
///
/// ```text
///   rev 1: def a():
///   rev 1:     pass
///   rev 2:
///   rev 2: def b():
///   rev 2:     pass
/// ```
///
/// In `rev 3`, insert a function. It could be either:
///
/// ```text
///   rev 1: def a():
///   rev 1:     pass
///   rev 3:
///   rev 3: def c():
///   rev 3:     pass
///   rev 2:
///   rev 2: def b():
///   rev 2:     pass
/// ```
///
/// Or (embed in rev 2, as if it depends on rev 2):
///
/// ```text
///   rev 1: def a():
///   rev 1:     pass
///   rev 2:
///   rev 3: def c():
///   rev 3:     pass
///   rev 3:
///   rev 2: def b():
///   rev 2:     pass
/// ```
///
/// Or (embed in rev 1, as if it depends on rev 1):
///
/// ```text
///   rev 1: def a():
///   rev 3:     pass
///   rev 3:
///   rev 3: def c():
///   rev 1:     pass
///   rev 2:
///   rev 2: def b():
///   rev 2:     pass
/// ```
#[test]
fn test_block_shift_effectiveness() {
    // For simplicity,  use the same `func_lines` (with multiple lines) for 3 functions.
    let text = "def f():\n    pass\n\n\n\n";
    let lines = text.lines().collect::<Vec<_>>();
    let n = lines.len();
    let expected_rev3_lines = lines.repeat(3);
    let expected_rev3_text = expected_rev3_lines.concat();
    let no_block_shift_flags = EditFlags::default() - EditFlags::BLOCK_SHIFT;

    // Rev 1: lines;  Rev 2: append lines.
    let base = AbstractLineLog::<&'static str>::default()
        .edit_chunk(E0, 0, 0, 0, 1, lines.clone(), no_block_shift_flags)
        .edit_chunk(E0, 1, n, n, 2, lines.clone(), no_block_shift_flags);

    let calculate_depends = |flags: EditFlags| -> Vec<String> {
        let mut grouped: BTreeMap<String, Vec<usize>> = Default::default();
        for a1 in 0..=(2 * n) {
            let lines = expected_rev3_lines[a1..a1 + n].to_vec();
            let log = base.clone().edit_chunk(E0, 2, a1, a1, 3, lines, flags);
            assert_eq!(log.checkout_text(E0, R(3)), expected_rev3_text);
            let dep = log.dep_dag();
            let dep = format!("DepMap({dep})");
            grouped.entry(dep).or_default().push(a1);
        }
        grouped.iter().map(|(k, v)| format!("{k}: {v:?}")).collect()
    };

    let depends = calculate_depends(no_block_shift_flags);
    assert_eq!(
        depends,
        [
            "DepMap({0,1,2,3}): [0, 5, 10]",
            "DepMap({0,1,2-3}): [6, 7, 8, 9]",
            "DepMap({0,1-3,2}): [1, 2, 3, 4]"
        ]
    );

    // With BLOCK_SHIFT (EditFlags::default), rev 1 or rev 2 aren't depended on.
    let flags = EditFlags::default();
    let depends = calculate_depends(flags);
    assert_eq!(
        depends,
        ["DepMap({0,1,2,3}): [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]"]
    );

    // BLOCK_SHIFT is enabled by default.
    assert!(EditFlags::default().contains(flags));
}

/// Test that block shift distance > len(insert_lines).
#[test]
fn test_block_shift_overflow() {
    // Insert into black lines.
    let base = AbstractLineLog::<&'static str>::default().edit_chunk(
        E0,
        0,
        0,
        0,
        1,
        vec!["", "", "", ""],
        EditFlags::default() - EditFlags::BLOCK_SHIFT,
    );

    for a1 in 0..4 {
        let log = base
            .clone()
            .edit_chunk(E0, 1, a1, a1, 1, vec![""], EditFlags::default());
        let dep = log.dep_dag();
        assert!(dep.iter().all(|(_rev, deps)| deps.is_empty()))
    }
}

#[test]
fn test_debug_nanodag() {
    let d = |edges: &[(Rev, Rev)]| -> String {
        let dag = NanoDag::from_edges(0, edges);
        format!("{dag:?}")
    };
    assert_eq!(d(&[]), "NanoDag()");
    assert_eq!(d(&[(3, 3)]), "NanoDag({0,1,2,3})");
    assert_eq!(d(&[(0, 1), (1, 2)]), "NanoDag(0-1-2)");
    assert_eq!(d(&[(0, 1), (2, 2)]), "NanoDag({0-1,2})");
    assert_eq!(d(&[(0, 2)]), "NanoDag({0-2,1})");
    assert_eq!(d(&[(0, 1), (0, 2)]), "NanoDag(0-{1,2})");
    assert_eq!(d(&[(0, 2), (1, 2)]), "NanoDag({0,1}-2)");

    // strange at first, but actually makes sense...
    assert_eq!(d(&[(0, 1), (1, 2), (0, 2)]), "NanoDag(0-{1,}-2)");

    // cross merge, some revs are duplicated
    assert_eq!(
        d(&[(0, 2), (0, 3), (1, 2), (1, 3)]),
        "NanoDag({0-{2,3},1-{2,3}})"
    );
    assert_eq!(
        d(&[(0, 1), (0, 2), (2, 4), (1, 3), (3, 4)]),
        "NanoDag(0-{1-3,2}-4)"
    );
    assert_eq!(
        d(&[(0, 1), (1, 2), (2, 5), (2, 3), (3, 4)]),
        "NanoDag(0-1-2-{3-4,5})",
    );

    // nested
    assert_eq!(
        d(&[(0, 1), (0, 2), (2, 3), (2, 4)]),
        "NanoDag(0-{1,2-{3,4}})"
    );
    assert_eq!(
        d(&[(0, 1), (0, 2), (2, 3), (2, 4), (3, 5), (4, 5), (1, 5)]),
        "NanoDag(0-{1,2-{3,4}}-5)"
    );
}

impl LineLog {
    fn show(&self, rev: usize) -> Vec<String> {
        self.checkout_lines(E0, R(rev))
            .into_iter()
            .map(|l| format!("{}:{}", l.rev, l.data.trim_end()))
            .collect()
    }

    fn show_range(&self, start: usize, end: usize) -> Vec<String> {
        let target_revs = SmallRevs::from_range(start..=end);
        self.checkout_lines(E0, CheckoutRev::Range(target_revs))
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
