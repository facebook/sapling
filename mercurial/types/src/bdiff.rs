/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

/// A single delta in a revlog or bundle.
///
/// The range from `start`-`end` is replaced with the `content`.
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Delta {
    pub start: usize,
    pub end: usize,
    pub content: Vec<u8>, // need to own because of compression
}

fn snip<T>(start: usize, end: usize, slice: &[T]) -> &[T] {
    let (h, _) = slice.split_at(end);
    let (_, t) = h.split_at(start);
    t
}

/// Apply a set of `Delta`s to an input text, returning the result.
pub fn apply(text: &[u8], deltas: &[Delta]) -> Vec<u8> {
    let mut chunks = Vec::with_capacity(deltas.len() * 2);
    let mut off = 0;

    for d in deltas {
        assert!(off <= d.start);
        if off < d.start {
            chunks.push(snip(off, d.start, text));
        }
        if d.content.len() > 0 {
            chunks.push(d.content.as_ref())
        }
        off = d.end;
    }
    if off < text.len() {
        chunks.push(snip(off, text.len(), text));
    }

    let mut ret = Vec::new();
    for s in chunks {
        ret.extend_from_slice(s);
    }
    ret
}

#[cfg(test)]
mod test {
    use super::{apply, Delta};

    #[test]
    fn test_1() {
        let text = b"aaaa\nbbbb\ncccc\n";
        let delta = Delta {
            start: 5,
            end: 10,
            content: (&b"xxxx\n"[..]).into(),
        };
        let deltas = [delta; 1];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"aaaa\nxxxx\ncccc\n");
    }

    #[test]
    fn test_2() {
        let text = b"bbbb\ncccc\n";
        let deltas = [
            Delta {
                start: 0,
                end: 5,
                content: (&b"aaaabbbb\n"[..]).into(),
            },
            Delta {
                start: 10,
                end: 10,
                content: (&b"dddd\n"[..]).into(),
            },
        ];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"aaaabbbb\ncccc\ndddd\n");
    }

    #[test]
    fn test_3a() {
        let text = b"aaaa\nbbbb\ncccc\n";
        let deltas = [Delta {
            start: 0,
            end: 15,
            content: (&b"zzzz\nyyyy\nxxxx\n"[..]).into(),
        }];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"zzzz\nyyyy\nxxxx\n");
    }

    #[test]
    fn test_3b() {
        let text = b"aaaa\nbbbb\ncccc\n";
        let deltas = [
            Delta {
                start: 0,
                end: 5,
                content: (&b"zzzz\n"[..]).into(),
            },
            Delta {
                start: 5,
                end: 10,
                content: (&b"yyyy\n"[..]).into(),
            },
            Delta {
                start: 10,
                end: 15,
                content: (&b"xxxx\n"[..]).into(),
            },
        ];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"zzzz\nyyyy\nxxxx\n");
    }

    #[test]
    fn test_4() {
        let text = b"aaaa\nbbbb";
        let deltas = [Delta {
            start: 5,
            end: 9,
            content: (&b"bbbbcccc"[..]).into(),
        }];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"aaaa\nbbbbcccc");
    }

    #[test]
    fn test_5() {
        let text = b"aaaa\nbbbb\ncccc\n";
        let deltas = [Delta {
            start: 5,
            end: 10,
            content: (&b""[..]).into(),
        }];

        let res = apply(text, &deltas[..]);
        assert_eq!(&res[..], b"aaaa\ncccc\n");
    }

}
