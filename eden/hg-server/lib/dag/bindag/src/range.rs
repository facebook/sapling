/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;

/// Range based on linear scan.
///
/// Ported from Mercurial's C code `reachableroots2()`.
///
/// The C implementation was added by Laurent Charignon on 2015-08-06 [1].
/// It was based on the a Python implementation added by Bryan O'Sullivan on
/// 2012-06-01 [2], which is similar to an older implementation by Eric Hopper
/// on 2005-10-07 [3], but faster and shorter.
///
/// The C code was then revised by others. The most significant change was
/// switching the "contains" check of "roots" and "reachable" from Python sets
/// to bits in the pure C "revstates" array for easier error handling and
/// better performance, by Yuya Nishihara on 2015-08-13 [4] [5].
///
/// Improvements in this Rust implementation:
/// - Use `VecDeque` for `tovisit` (roughly O(len(result)) -> O(len(heads))).
/// - Truncate `revstates` (O(len(changelog)) -> O(max_head - min_root)).
/// - Add `reachable.is_empty()` fast path that existed in the Python code.
/// - Support octopus merge.
///
/// [1]: https://www.mercurial-scm.org/repo/hg/rev/ff89383a97db
/// [2]: https://www.mercurial-scm.org/repo/hg/rev/b6efeb27e733
/// [3]: https://www.mercurial-scm.org/repo/hg/rev/518da3c3b6ce
/// [4]: https://www.mercurial-scm.org/repo/hg/rev/b68c9d232db6
/// [5]: https://www.mercurial-scm.org/repo/hg/rev/b3ad349d0e50
pub fn range(dag: &[impl AsRef<[usize]>], roots: &[usize], heads: &[usize]) -> Vec<usize> {
    if roots.is_empty() || heads.is_empty() {
        return Vec::new();
    }
    let min_root = *roots.iter().min().unwrap();
    let max_head = *heads.iter().max().unwrap();
    let len = max_head.max(min_root) - min_root + 1;
    let mut reachable = Vec::with_capacity(len);
    let mut tovisit = VecDeque::new();
    let mut revstates = vec![0u8; len];

    const RS_SEEN: u8 = 1;
    const RS_ROOT: u8 = 2;
    const RS_REACHABLE: u8 = 4;

    for &rev in roots {
        if rev <= max_head {
            revstates[rev - min_root] |= RS_ROOT;
        }
    }

    for &rev in heads {
        if rev >= min_root && revstates[rev - min_root] & RS_SEEN == 0 {
            tovisit.push_back(rev);
            revstates[rev - min_root] |= RS_SEEN;
        }
    }

    // Visit the tovisit list and find the reachable roots
    while let Some(rev) = tovisit.pop_front() {
        // Add the node to reachable if it is a root
        if revstates[rev - min_root] & RS_ROOT != 0 {
            revstates[rev - min_root] |= RS_REACHABLE;
            reachable.push(rev);
        }

        // Add its parents to the list of nodes to visit
        for &p in dag[rev].as_ref() {
            if p >= min_root && revstates[p - min_root] & RS_SEEN == 0 {
                tovisit.push_back(p);
                revstates[p - min_root] |= RS_SEEN;
            }
        }
    }

    if reachable.is_empty() {
        return Vec::new();
    }

    // Find all the nodes in between the roots we found and the heads
    // and add them to the reachable set
    for rev in min_root..=max_head {
        if revstates[rev - min_root] & RS_SEEN == 0 {
            continue;
        }
        if dag[rev]
            .as_ref()
            .iter()
            .any(|&p| p >= min_root && revstates[p - min_root] & RS_REACHABLE != 0)
            && revstates[rev - min_root] & RS_REACHABLE == 0
        {
            revstates[rev - min_root] |= RS_REACHABLE;
            reachable.push(rev);
        }
    }

    reachable
}
