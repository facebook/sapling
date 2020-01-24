/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// GCA based on linear scan.
///
/// Ported from Mercurial's C code `find_gca_candidates()`.
///
/// The algorithm was written by Bryan O'Sullivan on 2013-04-16.
/// He provided both a Python [1] and a C version [2]. Since then, the only real
/// logic change is removing an unnecessary "if" branch by Mads Kiilerich on
/// 2014-02-24 [4].
///
/// The C implementation is quite competitive among linear algorithms on
/// performance. It is cache-efficient, has fast paths to exit early, and
/// takes up to 62 (if bitmask is u64) revs at once. Other implementations
/// might just take 2 revs at most. For example, the older implemenation
/// by Matt Mackall in 2006 [3] takes 2 revs explicitly.
///
/// Changes in this Rust implementation:
/// - Change `bitmask` from `u64` to `u8` for smaller memory footage, at the
///   cost of losing support for more than 6 revs.
/// - Support octopus merge.
///
/// [1]: https://www.mercurial-scm.org/repo/hg/rev/2f7186400a07
/// [2]: https://www.mercurial-scm.org/repo/hg/rev/5bae936764bb
/// [3]: https://www.mercurial-scm.org/repo/hg/rev/b1db258e875c
/// [4]: https://www.mercurial-scm.org/repo/hg/rev/4add43865a9b
pub fn gca(dag: &[impl AsRef<[usize]>], revs: &[usize]) -> Vec<usize> {
    type BitMask = u8;
    let revcount = revs.len();
    assert!(revcount < 7);
    if revcount == 0 {
        return Vec::new();
    }

    let allseen: BitMask = (1 << revcount) - 1;
    let poison: BitMask = 1 << revcount;
    let maxrev = revs.iter().max().cloned().unwrap();
    let mut interesting = revcount;
    let mut gca = Vec::new();
    let mut seen: Vec<BitMask> = vec![0; maxrev + 1];

    for (i, &rev) in revs.iter().enumerate() {
        seen[rev] = 1 << i;
    }

    for v in (0..=maxrev).rev() {
        if interesting == 0 {
            break;
        }
        let mut sv = seen[v];
        if sv == 0 {
            continue;
        }
        if sv < poison {
            interesting -= 1;
            if sv == allseen {
                gca.push(v);
                sv |= poison;
                if revs.iter().any(|&r| r == v) {
                    break;
                }
            }
        }
        for &p in dag[v].as_ref() {
            let sp = seen[p];
            if sv < poison {
                if sp == 0 {
                    seen[p] = sv;
                    interesting += 1
                } else if sp != sv {
                    seen[p] |= sv
                }
            } else {
                if sp != 0 && sp < poison {
                    interesting -= 1
                }
                seen[p] = sv
            }
        }
    }

    gca
}
