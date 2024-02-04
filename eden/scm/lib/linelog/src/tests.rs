/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use im::Vector as ImVec;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaChaRng;

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

fn lines(s: &str) -> Vec<String> {
    s.lines().map(|s| format!("{}\n", s)).collect()
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
