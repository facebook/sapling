/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

pub struct HeaderlessDiffOpts {
    /// Number of context lines
    pub context: usize,
}

pub enum CopyInfo {
    /// File was modified, added or removed.
    None,
    /// File was moved.
    Move,
    /// File was copied.
    Copy,
}

pub struct DiffOpts {
    /// Number of context lines
    pub context: usize,
    pub copy_info: CopyInfo,
}

const MISSING_NEWLINE_MARKER: &[u8] = b"\\ No newline at end of file\n";

struct DiffPayload<'a, 'b> {
    old_has_trailing_newline: bool,
    new_has_trailing_newline: bool,
    old_lines: Vec<&'a [u8]>,
    new_lines: Vec<&'b [u8]>,
}

struct DiffState<S, F>
where
    F: Fn(S, &[u8]) -> S,
{
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

    fn emit_binary_has_changed(&mut self, path: &[u8]) {
        self.emit(b"Binary file ");
        self.emit(path);
        self.emit(b" has changed\n");
    }

    fn emit_binary_files_differ(&mut self, old_path: &[u8], new_path: &[u8]) {
        self.emit(b"Binary files a/");
        self.emit(old_path);
        self.emit(b" and b/");
        self.emit(new_path);
        self.emit(b" differ\n");
    }

    pub fn collect(self) -> S {
        self.seed.unwrap()
    }

    pub fn unwrap(self) -> (F, S) {
        (self.reduce, self.seed.unwrap())
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
    opts: HeaderlessDiffOpts,
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

/// A helper function that allow us to avoid calling xdiff for trivial all-plus
/// and all-minus diffs.
///
/// `emit` callback is called many times as the parts of diff are generated
///
/// Hopefuly we'll be able to refactor this API to use (currently unstable) Rust generators.
/// The only available public API now is `diff_unified` which just returns a `vec<u8>`.
fn gen_diff_unified_headerless_entire_file<T, F, S>(prefix: u8, text: &T, seed: S, reduce: F) -> S
where
    T: AsRef<[u8]>,
    F: Fn(S, &[u8]) -> S,
{
    let mut state = DiffState {
        seed: Some(seed),
        reduce,
    };
    let text = text.as_ref();
    let missing_newline = !text.is_empty() && !text.ends_with(b"\n");
    let text_to_split = if missing_newline || text.is_empty() {
        &text[..]
    } else {
        &text[0..text.len() - 1]
    };
    let start = if text.is_empty() { 0 } else { 1 };
    let count = text_to_split.split(|c| c == &b'\n').count();
    state.emit(
        format!(
            "@@ -{},{} +{},{} @@\n",
            if prefix == b'-' { start } else { 0 },
            if prefix == b'-' { count } else { 0 },
            if prefix == b'+' { start } else { 0 },
            if prefix == b'+' { count } else { 0 },
        )
        .as_bytes(),
    );
    text_to_split.split(|c| c == &b'\n').for_each(|line| {
        state.emit(&[prefix]);
        state.emit(line);
        state.emit(b"\n");
    });
    if missing_newline {
        state.emit(MISSING_NEWLINE_MARKER);
    }
    state.collect()
}

/// Computes a headerless diff between two byte-slices: `old_text` and `new_text`,
/// the number of `context` lines can be set in `opts` struct.
///
/// Returns a vector of bytes containing the entire diff.
pub fn diff_unified_headerless<T>(old_text: &T, new_text: &T, opts: HeaderlessDiffOpts) -> Vec<u8>
where
    T: AsRef<[u8]>,
{
    gen_diff_unified_headerless(old_text, new_text, opts, Vec::new(), |mut v, part| {
        v.extend(part);
        v
    })
}

#[derive(Debug, Copy, Clone)]
pub enum FileType {
    Regular,
    Executable,
    Symlink,
}

/// Struct representing the diffed file. Contains all the information
/// needed for header-generation.
#[derive(Debug, Copy, Clone)]
pub struct DiffFile<P, C>
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    /// file path (as [u8])
    pub path: P,
    /// file contents (as [u8])
    pub contents: C,
    pub file_type: FileType,
}

impl<P, C> DiffFile<P, C>
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    pub fn new(path: P, contents: C, file_type: FileType) -> Self {
        Self {
            path,
            contents,
            file_type,
        }
    }
}

fn file_is_binary<P, C>(file: &Option<DiffFile<P, C>>) -> bool
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    match file {
        Some(file) => file.contents.as_ref().contains(&b'\0'),
        None => false,
    }
}

/// Computes a diff between two files `old_file` and `new_file`,
/// the number of `context` lines can be set in `opts` struct.
///
/// In case of binary files just emits a placeholder.
///
/// `emit` callback is called many times as the parts of diff are generated.
fn gen_diff_unified<P, C, F, S>(
    old_file: Option<DiffFile<P, C>>,
    new_file: Option<DiffFile<P, C>>,
    diff_opts: DiffOpts,
    seed: S,
    reduce: F,
) -> S
where
    P: AsRef<[u8]>,
    P: Clone,
    C: AsRef<[u8]>,
    F: Fn(S, &[u8]) -> S,
{
    let mut state = DiffState {
        seed: Some(seed),
        reduce,
    };
    fn file_type_to_mode(file_type: FileType) -> &'static [u8] {
        match file_type {
            FileType::Executable => b"100755",
            FileType::Symlink => b"120000",
            FileType::Regular => b"100644",
        }
    }
    if let (None, None) = (&old_file, &new_file) {
        return state.collect();
    }
    // When the files have no differences the output should be empty.
    if let (Some(old_file), Some(new_file)) = (&old_file, &new_file) {
        let old_contents = old_file.contents.as_ref();
        let new_contents = new_file.contents.as_ref();
        if old_contents.len() == new_contents.len() && old_contents == new_contents {
            return state.collect();
        }
    }
    let old_name = &(old_file.as_ref()).or((&new_file).as_ref()).unwrap().path;
    let new_name = &(new_file.as_ref()).or((&old_file).as_ref()).unwrap().path;
    state.emit(b"diff --git a/");
    state.emit(old_name.as_ref());
    state.emit(b" b/");
    state.emit(new_name.as_ref());
    state.emit(b"\n");
    match (&old_file, &new_file) {
        (None, Some(new_file)) => {
            state.emit(b"new file mode ");
            state.emit(file_type_to_mode(new_file.file_type));
            state.emit(b"\n");
        }
        (Some(old_file), None) => {
            state.emit(b"deleted file mode ");
            state.emit(file_type_to_mode(old_file.file_type));
            state.emit(b"\n");
        }
        (Some(old_file), Some(new_file)) => {
            if file_type_to_mode(old_file.file_type) != file_type_to_mode(new_file.file_type) {
                state.emit(b"old mode ");
                state.emit(file_type_to_mode(old_file.file_type));
                state.emit(b"\n");
                state.emit(b"new mode ");
                state.emit(file_type_to_mode(new_file.file_type));
                state.emit(b"\n");
            }
            match diff_opts.copy_info {
                CopyInfo::Move => {
                    state.emit(b"rename from ");
                    state.emit(&old_file.path.as_ref());
                    state.emit(b"\n");
                    state.emit(b"rename to ");
                    state.emit(&new_file.path.as_ref());
                    state.emit(b"\n");
                }
                CopyInfo::Copy => {
                    state.emit(b"copy from ");
                    state.emit(&old_file.path.as_ref());
                    state.emit(b"\n");
                    state.emit(b"copy to ");
                    state.emit(&new_file.path.as_ref());
                    state.emit(b"\n");
                }
                CopyInfo::None => {}
            }
        }
        // Impossible here.
        (None, None) => (),
    }
    // Header for binary files
    if file_is_binary(&old_file) || file_is_binary(&new_file) {
        match (old_file, new_file) {
            (Some(old_file), None) => state.emit_binary_has_changed(old_file.path.as_ref()),
            (None, Some(new_file)) => state.emit_binary_has_changed(new_file.path.as_ref()),
            (Some(old_file), Some(new_file)) => {
                if old_file.path.as_ref() == new_file.path.as_ref() {
                    state.emit_binary_has_changed(new_file.path.as_ref())
                } else {
                    state.emit_binary_files_differ(old_file.path.as_ref(), new_file.path.as_ref())
                }
            }
            _ => (),
        }
        return state.collect();
    }

    // Headers for old file.
    if let Some(old_file) = &old_file {
        state.emit(b"--- a/");
        state.emit(old_file.path.as_ref());
        state.emit(b"\n");
    } else {
        state.emit(b"--- /dev/null\n");
    }
    // Headers for new file.
    if let Some(new_file) = &new_file {
        state.emit(b"+++ b/");
        state.emit(new_file.path.as_ref());
        state.emit(b"\n");
    } else {
        state.emit(b"+++ /dev/null\n");
    }
    // All headers emitted, now emit the actual diff.
    let (reduce, seed) = state.unwrap();
    match (&old_file, &new_file) {
        (Some(old_file), Some(new_file)) => {
            // Typical case, we need to call actual diff function to get the diff.
            let opts = HeaderlessDiffOpts {
                context: diff_opts.context,
            };
            gen_diff_unified_headerless(&old_file.contents, &new_file.contents, opts, seed, reduce)
        }
        (Some(old_file), None) => {
            // Degenerated case of all-minus diff.
            gen_diff_unified_headerless_entire_file(b'-', &old_file.contents, seed, reduce)
        }
        (None, Some(new_file)) => {
            // Degenerated case of all-plus diff.
            gen_diff_unified_headerless_entire_file(b'+', &new_file.contents, seed, reduce)
        }
        (None, None) => {
            // There's nothing to diff.
            seed
        }
    }
}

/// Computes a diff between two files `old_file` and `new_file`,
/// the number of `context` lines can be set in `opts` struct.
///
/// Returns a vector of bytes containing the entire diff.
pub fn diff_unified<N, C>(
    old_file: Option<DiffFile<N, C>>,
    new_file: Option<DiffFile<N, C>>,
    opts: DiffOpts,
) -> Vec<u8>
where
    N: AsRef<[u8]> + Clone,
    C: AsRef<[u8]>,
{
    gen_diff_unified(old_file, new_file, opts, Vec::new(), |mut v, part| {
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
            diff_unified_headerless(&a, &b, HeaderlessDiffOpts { context: 10 }),
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

    #[test]
    fn test_diff_unified() {
        let a = r#"a
b
c
d1
d2
z"#;
        let b = r#"a
b2
b3
c
d
e
z"#;
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: &a,
                    path: "x",
                    file_type: FileType::Regular,
                }),
                Some(DiffFile {
                    contents: &b,
                    path: "y",
                    file_type: FileType::Regular,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/y
--- a/x
+++ b/y
@@ -1,6 +1,7 @@
 a
-b
+b2
+b3
 c
-d1
-d2
+d
+e
 z
\ No newline at end of file
"
        );
    }

    #[test]
    fn test_diff_unified_with_empty() {
        let a = r#"a
b
c
d
"#;
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: &a,
                    path: "x",
                    file_type: FileType::Regular,
                }),
                None,
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
deleted file mode 100644
--- a/x
+++ /dev/null
@@ -1,4 +0,0 @@
-a
-b
-c
-d
"
        );
    }
}
