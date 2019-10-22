// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Functions to find the difference between two texts.
//! Under the hood it's using the xdiff library that's also used by git and hg.

use std::cmp::min;
use std::ops::Range;
use std::os::raw::{c_char, c_int, c_void};
use xdiff_sys as ffi;

/// An individual difference between two texts. Consists of two
/// line ranges that specify which parts of the texts differ.
///
/// If any of the ranges is empty it's still significant because it
/// specifies the location in the text where the other range applies.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// Line range from new text to insert
    pub add: Range<usize>,
    /// Line from the old text to delete
    pub remove: Range<usize>,
}

/// Computes the hunks of differerences between two texts.
///
/// Takes two byte-slices as arguments.
/// Returns a list of hunks that indicate differences between the old and new text.
///
/// # Example
/// ```
/// use xdiff::{diff_hunks, Hunk};
/// let a = "a\n b\n c\n d\n";
/// let b = "a\n c\n d\n e\n";
/// let c = "a\n b\n c\n d\n e\n";
/// assert_eq!(
///     diff_hunks(a, b),
///     [
///         Hunk {
///             add: 1..1,
///             remove: 1..2,
///         },
///         Hunk {
///             add: 3..4,
///             remove: 4..4,
///         }
///     ]
/// );
/// assert_eq!(
///     diff_hunks(a, c),
///     [Hunk {
///         add: 4..5,
///         remove: 4..4
///     }]
/// );
/// let x = diff_hunks(b, c);
/// assert_eq!(x[0].add.end, 2);
/// ```
pub fn diff_hunks<T>(old_text: T, new_text: T) -> Vec<Hunk>
where
    T: AsRef<[u8]>,
{
    extern "C" fn hunk_consumer(a1: i64, a2: i64, b1: i64, b2: i64, _priv: *mut c_void) -> c_int {
        let mut _priv = unsafe { (_priv as *mut Vec<Hunk>).as_mut() };
        let a1 = a1 as usize;
        let a2 = a2 as usize;
        let b1 = b1 as usize;
        let b2 = b2 as usize;
        if let Some(result) = _priv {
            result.push(Hunk {
                add: (b1..(b1 + b2)),
                remove: (a1..(a1 + a2)),
            });
        }
        return 0;
    }

    let old_text = old_text.as_ref();
    let mut old_mmfile = ffi::mmfile_t {
        ptr: old_text.as_ptr() as *const c_char,
        size: old_text.len() as i64,
    };
    let new_text = new_text.as_ref();
    let mut new_mmfile = ffi::mmfile_t {
        ptr: new_text.as_ptr() as *const c_char,
        size: new_text.len() as i64,
    };
    let xpp = ffi::xpparam_t { flags: 0 };
    let xecfg = ffi::xdemitconf_t {
        flags: 0,
        hunk_func: Some(hunk_consumer),
    };
    let mut result: Vec<Hunk> = Vec::new();
    let mut ecb = ffi::xdemitcb_t {
        priv_: &mut result as *mut Vec<Hunk> as *mut c_void,
    };

    unsafe {
        ffi::xdl_diff(&mut old_mmfile, &mut new_mmfile, &xpp, &xecfg, &mut ecb);
    }
    return result;
}

pub struct DiffOpts {
    /// Number of context lines
    pub context: usize,
}

const MISSING_NEWLINE_MARKER: &[u8] = b"\\ No newline at end of file\n";

struct DiffPayload<'a, 'b> {
    old_has_trailing_newline: bool,
    new_has_trailing_newline: bool,
    old_lines: Vec<&'a [u8]>,
    new_lines: Vec<&'b [u8]>,
}

struct DiffState<S, F> {
    seed: Option<S>,
    reduce: F,
}

impl<S, F> DiffState<S, F>
where
    F: Fn(S, &[u8]) -> S,
{
    fn emit_line(&mut self, prefix: &[u8], line: &[u8]) {
        self.emit(prefix);
        self.emit(line);
        self.emit(b"\n");
    }

    fn emit(&mut self, text: &[u8]) {
        // This option can be unwrapped since it'll never be None: we always put something back in
        // its place.
        let next = (self.reduce)(self.seed.take().unwrap(), text);
        self.seed = Some(next)
    }

    pub fn emit_hunk_cluster(
        &mut self,
        payload: &DiffPayload,
        cluster_bounds: Hunk,
        included_hunks: &[Hunk],
    ) {
        // Emit the header.
        self.emit(
            format!(
                "@@ -{},{} +{},{} @@\n",
                // Of course line ranges in the diff format start from 1.
                &cluster_bounds.remove.start + 1,
                &cluster_bounds.remove.len(),
                &cluster_bounds.add.start + 1,
                &cluster_bounds.add.len()
            )
            .as_bytes(),
        );

        let mut previous_hunk: Option<&Hunk> = None;

        for hunk in included_hunks {
            let context_start = previous_hunk
                .map(|h| h.remove.end)
                .unwrap_or(cluster_bounds.remove.start);
            payload.old_lines[context_start..hunk.remove.start]
                .iter()
                .for_each(|line| self.emit_line(b" ", *line));

            // Emit the lines from the old file preceded by '-' char.
            payload.old_lines[hunk.remove.clone()]
                .iter()
                .for_each(|line| {
                    self.emit_line(b"-", *line);
                });
            // In case file ends without newline we need to print a warning about it.
            if hunk.remove.end == payload.old_lines.len() {
                if !payload.old_has_trailing_newline {
                    self.emit(MISSING_NEWLINE_MARKER);
                }
            }
            // Emit the lines from the new file preceded by '+' char.
            payload.new_lines[hunk.add.clone()].iter().for_each(|line| {
                self.emit_line(b"+", *line);
            });
            // In case file ends without newline we need to print a warning about it.
            if hunk.add.end == payload.new_lines.len() {
                if !payload.new_has_trailing_newline {
                    self.emit(MISSING_NEWLINE_MARKER);
                }
            }
            previous_hunk = Some(hunk);
        }
        // After the last chunk emit the remaining context.
        if let Some(previous_hunk) = previous_hunk {
            payload.old_lines[previous_hunk.remove.end..cluster_bounds.remove.end]
                .iter()
                .for_each(|line| {
                    self.emit_line(b" ", *line);
                });
            // If the last line is the same in both files and it's missing newline we print the marker
            if cluster_bounds.remove.end == payload.old_lines.len()
                && previous_hunk.remove.end != cluster_bounds.remove.end
            {
                if !payload.old_has_trailing_newline {
                    self.emit(MISSING_NEWLINE_MARKER);
                }
            }
        }
    }

    pub fn collect(self) -> S {
        self.seed.unwrap()
    }
}

/// Computes a headerless diff between two byte-slices: `old_text` and `new_text`,
/// the number of `context` lines can be set in `opts` struct.
///
/// `emit` callback is called many times as the parts of diff are generated
///
/// Hopefuly we'll be able to refactor this API to use (currently unstable) Rust generators.
/// The only available public API now is `diff_unified_headerless` which just returns a `vec<u8>`.
fn gen_diff_unified_headerless<T, F, S>(
    old_text: &T,
    new_text: &T,
    opts: DiffOpts,
    seed: S,
    reduce: F,
) -> S
where
    T: AsRef<[u8]>,
    F: Fn(S, &[u8]) -> S,
{
    let old_text = old_text.as_ref();
    let new_text = new_text.as_ref();
    let mut old_lines: Vec<_> = old_text.split(|c| c == &b'\n').collect();
    let mut new_lines: Vec<_> = new_text.split(|c| c == &b'\n').collect();

    let old_has_trailing_newline = old_text.last() == Some(&b'\n');
    let new_has_trailing_newline = new_text.last() == Some(&b'\n');

    // The last empty line traditionally doesn't count as line. Not having it is a case for warnings.
    if old_has_trailing_newline {
        old_lines.pop();
    }
    if new_has_trailing_newline {
        new_lines.pop();
    }
    let hunks = diff_hunks(old_text, new_text);

    if hunks.is_empty() {
        return seed;
    }

    let payload = DiffPayload {
        old_lines,
        new_lines,
        old_has_trailing_newline,
        new_has_trailing_newline,
    };

    // TODO: Expose a constructor for this so that it cannot be constructed wiht seed: None.
    let mut state = DiffState {
        seed: Some(seed),
        reduce,
    };

    // Helpers for adding and removing context to the line numbers while keeping them
    // within array bounds.
    let sub_context = |n: usize| n.saturating_sub(opts.context);
    let add_context = |n: usize, max_l: usize| min(n + opts.context, max_l);

    // Helper for emitting a single hunk cluster.
    // (a group of chunks with overlapping contexts and shared header)

    let mut cluster: Option<(Hunk, Range<usize>)> = None;
    for (hunk_no, hunk) in hunks.iter().enumerate() {
        // `clusters` is vector of tuples (cluster_bounds, hunk_range) where:
        //  - cluster_bounds contains the line ranges the new and old file covered
        //    by cluster (including context)
        //  - hunk range contains the range of individual hunks from the `hunks` vector that
        //    are included
        // Check for overlap with the previous hunk:
        cluster = match cluster {
            Some((ref cluster_bounds, ref included_hunks_range))
                if hunks[hunk_no - 1].remove.end + opts.context
                    >= hunk.remove.start.saturating_sub(opts.context) =>
            {
                // Overlap with previous hunk, merging the bounds and extending the range.
                Some((
                    Hunk {
                        remove: cluster_bounds.remove.start
                            ..add_context(hunk.remove.end, payload.old_lines.len()),
                        add: cluster_bounds.add.start
                            ..add_context(hunk.add.end, payload.new_lines.len()),
                    },
                    included_hunks_range.start..hunk_no + 1,
                ))
            }
            _ => {
                // No overlap with previous hunk. Emit current cluster and start a new one.
                if let Some((cluster_bounds, included_hunks_range)) = cluster {
                    state.emit_hunk_cluster(&payload, cluster_bounds, &hunks[included_hunks_range]);
                }
                Some((
                    Hunk {
                        remove: sub_context(hunk.remove.start)
                            ..add_context(hunk.remove.end, payload.old_lines.len()),
                        add: sub_context(hunk.add.start)
                            ..add_context(hunk.add.end, payload.new_lines.len()),
                    },
                    hunk_no..hunk_no + 1,
                ))
            }
        };
    }
    // Emit the last cluster.
    if let Some((cluster_bounds, included_hunks_range)) = cluster {
        state.emit_hunk_cluster(&payload, cluster_bounds, &hunks[included_hunks_range]);
    }

    state.collect()
}

/// Computes a headerless diff between two byte-slices: `old_text` and `new_text`,
/// the number of `context` lines can be set in `opts` struct.
///
/// Returns a vector of bytes containing the entire diff.
pub fn diff_unified_headerless<T>(old_text: &T, new_text: &T, opts: DiffOpts) -> Vec<u8>
where
    T: AsRef<[u8]>,
{
    gen_diff_unified_headerless(old_text, new_text, opts, Vec::new(), |mut v, part| {
        v.extend(part);
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks() {
        // We're repeating the example from doctest because "buck test" doesn't run it.
        let a = "a\n b\n c\n d\n";
        let b = "a\n c\n d\n e\n";
        let c = "a\n b\n c\n d\n e\n";
        assert_eq!(
            diff_hunks(a, b),
            [
                Hunk {
                    add: 1..1,
                    remove: 1..2,
                },
                Hunk {
                    add: 3..4,
                    remove: 4..4,
                }
            ]
        );
        assert_eq!(
            diff_hunks(a, c),
            [Hunk {
                add: 4..5,
                remove: 4..4
            }]
        );
        let x = diff_hunks(b, c);
        assert_eq!(x[0].add.end, 2);
    }

    #[test]
    fn test_diff_unified_headerless() {
        let a = r#"a
b
c
d
"#;
        let b = r#"a
c
d
e
z"#;
        assert_eq!(
            diff_unified_headerless(&a, &b, DiffOpts { context: 10 }),
            r"@@ -1,4 +1,5 @@
 a
-b
 c
 d
+e
+z
\ No newline at end of file
"
            .as_bytes()
        );
    }
}
