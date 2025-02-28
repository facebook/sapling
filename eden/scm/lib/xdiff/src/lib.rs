/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Functions to find the difference between two texts.
//! Under the hood it's using the xdiff library that's also used by git and hg.

use std::cmp::min;
use std::ops::Range;
use std::os::raw::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;

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
/// use xdiff::diff_hunks;
/// use xdiff::Hunk;
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
        0
    }

    let old_text = old_text.as_ref();
    let mut old_mmfile = ffi::mmfile_t {
        ptr: old_text.as_ptr() as *mut c_char,
        size: old_text.len() as i64,
    };
    let new_text = new_text.as_ref();
    let mut new_mmfile = ffi::mmfile_t {
        ptr: new_text.as_ptr() as *mut c_char,
        size: new_text.len() as i64,
    };
    let xpp = ffi::xpparam_t {
        flags: ffi::XDF_INDENT_HEURISTIC as u64,
        ..Default::default()
    };
    let xecfg = ffi::xdemitconf_t {
        flags: 0,
        hunk_func: Some(hunk_consumer),
    };
    let mut result: Vec<Hunk> = Vec::new();
    let mut ecb = ffi::xdemitcb_t {
        priv_: &mut result as *mut Vec<Hunk> as *mut c_void,
    };

    unsafe {
        ffi::xdl_diff_vendored(&mut old_mmfile, &mut new_mmfile, &xpp, &xecfg, &mut ecb);
    }

    result
}

/// Produce matching blocks, in (a1, a2, b1, b2) format.
/// `a_lines[a1:a2]` matches `b_lines[b1:b2]`.
pub fn blocks(a: &[u8], b: &[u8]) -> Vec<(u64, u64, u64, u64)> {
    extern "C" fn hunk_consumer(a1: i64, a2: i64, b1: i64, b2: i64, blocks: *mut c_void) -> c_int {
        let blocks = unsafe { (blocks as *mut Vec<(u64, u64, u64, u64)>).as_mut() };
        if let Some(blocks) = blocks {
            blocks.push((a1 as _, a2 as _, b1 as _, b2 as _));
        }
        0
    }

    let mut a_mmfile = ffi::mmfile_t {
        ptr: a.as_ptr() as *mut c_char,
        size: a.len() as i64,
    };
    let mut b_mmfile = ffi::mmfile_t {
        ptr: b.as_ptr() as *mut c_char,
        size: b.len() as i64,
    };
    let xpp = ffi::xpparam_t {
        flags: ffi::XDF_INDENT_HEURISTIC as u64,
        ..Default::default()
    };
    let xecfg = ffi::xdemitconf_t {
        flags: ffi::XDL_EMIT_BDIFFHUNK as _,
        hunk_func: Some(hunk_consumer),
    };
    let mut result: Vec<(u64, u64, u64, u64)> = Vec::new();
    let mut ecb = ffi::xdemitcb_t {
        priv_: &mut result as *mut Vec<(u64, u64, u64, u64)> as *mut c_void,
    };

    let ret =
        unsafe { ffi::xdl_diff_vendored(&mut a_mmfile, &mut b_mmfile, &xpp, &xecfg, &mut ecb) };

    assert_eq!(ret, 0, "xdl_diff failed");

    result
}

/// Calculate the edit cost (added and deleted line count), with a maximum threshold.
///
/// The maximum threshold `max_edit_cost` decides the maximum D in O(N+D^2)
/// complexity. Do not set it to a large value. The underlying diff algorithm
/// does not use any heuristics!
///
/// This is useful for similarity check.
pub fn edit_cost(a: &[u8], b: &[u8], max_edit_cost: u64) -> u64 {
    let mut a_mmfile = ffi::mmfile_t {
        ptr: a.as_ptr() as *mut c_char,
        size: a.len() as i64,
    };
    let mut b_mmfile = ffi::mmfile_t {
        ptr: b.as_ptr() as *mut c_char,
        size: b.len() as i64,
    };
    let xpp = ffi::xpparam_t {
        flags: ffi::XDF_CAPPED_EDIT_COST_ONLY as _,
        max_edit_cost: max_edit_cost as _,
    };
    let xecfg = ffi::xdemitconf_t {
        flags: 0,
        hunk_func: None,
    };

    let ret = unsafe {
        ffi::xdl_diff_vendored(
            &mut a_mmfile,
            &mut b_mmfile,
            &xpp,
            &xecfg,
            std::ptr::null_mut(),
        )
    };

    assert!(ret >= 0, "xdl_diff failed");

    ret as _
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeaderlessDiffOpts {
    /// Number of context lines
    pub context: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CopyInfo {
    /// File was modified, added or removed.
    None,
    /// File was moved.
    Move,
    /// File was copied.
    Copy,
}

#[derive(Clone, PartialEq, Eq, Debug)]
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
    fn new(seed: S, reduce: F) -> Self {
        Self {
            seed: Some(seed),
            reduce,
        }
    }

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
                // Increment since line numbers in a diff start from 1. If a
                // hunk contains zero lines, the line number has to be one lower
                // than one would expect. So, don't increment in that case.
                cluster_bounds.remove.start
                    + (if cluster_bounds.remove.is_empty() {
                        0
                    } else {
                        1
                    }),
                &cluster_bounds.remove.len(),
                cluster_bounds.add.start + (if cluster_bounds.add.is_empty() { 0 } else { 1 }),
                &cluster_bounds.add.len(),
            )
            .as_bytes(),
        );

        let mut previous_hunk: Option<&Hunk> = None;

        for hunk in included_hunks {
            let context_start = previous_hunk.map_or(cluster_bounds.remove.start, |h| h.remove.end);
            payload.old_lines[context_start..hunk.remove.start]
                .iter()
                .for_each(|line| self.emit_line(b" ", line));

            // Emit the lines from the old file preceded by '-' char.
            payload.old_lines[hunk.remove.clone()]
                .iter()
                .for_each(|line| {
                    self.emit_line(b"-", line);
                });
            // In case file ends without newline we need to print a warning about it.
            if hunk.remove.end == payload.old_lines.len() {
                if !payload.old_has_trailing_newline {
                    self.emit(MISSING_NEWLINE_MARKER);
                }
            }
            // Emit the lines from the new file preceded by '+' char.
            payload.new_lines[hunk.add.clone()].iter().for_each(|line| {
                self.emit_line(b"+", line);
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
                    self.emit_line(b" ", line);
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

    fn emit_submodule_changes(
        &mut self,
        old_commit_hash: Option<&str>,
        new_commit_hash: Option<&str>,
    ) {
        if old_commit_hash.is_some() {
            self.emit(b"@@ -1 ");
        } else {
            self.emit(b"@@ -0,0 ");
        }
        if new_commit_hash.is_some() {
            self.emit(b"+1 @@\n");
        } else {
            self.emit(b"+0,0 @@\n");
        }
        if let Some(hash) = old_commit_hash {
            self.emit(format!("-Subproject commit {hash}\n").as_bytes());
        }
        if let Some(hash) = new_commit_hash {
            self.emit(format!("+Subproject commit {hash}\n").as_bytes());
        }
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
/// Hopefully we'll be able to refactor this API to use (currently unstable) Rust generators.
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

    // No trailing newline in empty files is expected, for any other file a warning is displayed.
    let old_has_trailing_newline = old_text.is_empty() || old_text.last() == Some(&b'\n');
    let new_has_trailing_newline = new_text.is_empty() || new_text.last() == Some(&b'\n');

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

    let mut state = DiffState::new(seed, reduce);

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
/// Hopefully we'll be able to refactor this API to use (currently unstable) Rust generators.
/// The only available public API now is `diff_unified` which just returns a `vec<u8>`.
fn gen_diff_unified_headerless_entire_file<T, F, S>(prefix: u8, text: &T, seed: S, reduce: F) -> S
where
    T: AsRef<[u8]>,
    F: Fn(S, &[u8]) -> S,
{
    let mut state = DiffState::new(seed, reduce);
    let text = text.as_ref();
    let missing_newline = !text.is_empty() && !text.ends_with(b"\n");
    let text_to_split = if missing_newline || text.is_empty() {
        text
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Regular,
    Executable,
    Symlink,
    GitSubmodule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileContent<C>
where
    C: AsRef<[u8]>,
{
    /// The file content was not fetched (for example, if it's too large), though we still want
    /// to produce a placeholder diff for it.
    Omitted {
        /// The hash of the file contents.
        content_hash: String,
    },
    Inline(C),
    /// The file refers to a submodule, and a corresponding placeholder should be generated for
    /// it.
    Submodule {
        /// The commit hash of the submodule commit.
        commit_hash: String,
    },
}

impl<C: AsRef<[u8]>> FileContent<C> {
    fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            FileContent::Omitted { .. } | FileContent::Submodule { .. } => None,
            FileContent::Inline(c) => Some(c.as_ref()),
        }
    }

    fn is_omitted(&self) -> bool {
        match self {
            FileContent::Omitted { .. } => true,
            _ => false,
        }
    }

    fn submodule_commit_hash(&self) -> Option<&str> {
        match self {
            FileContent::Submodule { commit_hash } => Some(commit_hash),
            _ => None,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            FileContent::Inline(c) => c.as_ref().is_empty(),
            _ => false,
        }
    }
}

/// Struct representing the diffed file. Contains all the information
/// needed for header-generation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffFile<P, C>
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    /// file path (as [u8])
    pub path: P,
    /// file contents (as [u8])
    pub contents: FileContent<C>,
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
            contents: FileContent::Inline(contents),
            file_type,
        }
    }
}

pub fn file_is_binary<P, C>(file: &Option<DiffFile<P, C>>) -> bool
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    match file {
        Some(file) => file
            .contents
            .as_bytes()
            .map_or(false, |bytes| bytes.contains(&0)),
        None => false,
    }
}

fn file_is_omitted<P, C>(file: &Option<DiffFile<P, C>>) -> bool
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    match file {
        Some(file) => file.contents.is_omitted(),
        None => false,
    }
}

fn file_is_submodule<P, C>(file: &Option<DiffFile<P, C>>) -> bool
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    match file {
        Some(file) => file.file_type == FileType::GitSubmodule,
        _ => false,
    }
}

fn file_is_empty<P, C>(file: &Option<DiffFile<P, C>>) -> bool
where
    P: AsRef<[u8]>,
    C: AsRef<[u8]>,
{
    match file {
        Some(file) => file.contents.is_empty(),
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
    C: AsRef<[u8]> + PartialEq + Eq,
    F: Fn(S, &[u8]) -> S + Clone,
{
    fn file_type_to_mode(file_type: FileType) -> &'static [u8] {
        match file_type {
            FileType::Executable => b"100755",
            FileType::Symlink => b"120000",
            FileType::Regular => b"100644",
            FileType::GitSubmodule => b"160000",
        }
    }

    // If the file is changed to or from a git submodule, this always
    // shows up as a delete followed by an add.
    if let (Some(old), Some(new)) = (&old_file, &new_file) {
        if (old.file_type == FileType::GitSubmodule) != (new.file_type == FileType::GitSubmodule) {
            let seed = gen_diff_unified(old_file, None, diff_opts.clone(), seed, reduce.clone());
            let seed = gen_diff_unified(None, new_file, diff_opts, seed, reduce);
            return seed;
        }
    }

    let mut state = DiffState::new(seed, reduce);

    if let (None, None) = (&old_file, &new_file) {
        return state.collect();
    }

    // When the files have no content differences and no metadata differences the output should be empty.
    if let (Some(old_file), Some(new_file)) = (&old_file, &new_file) {
        if old_file.file_type == new_file.file_type && diff_opts.copy_info == CopyInfo::None {
            if old_file.contents == new_file.contents {
                return state.collect();
            }
        }
    }

    let old_name = &(old_file.as_ref()).or(new_file.as_ref()).unwrap().path;
    let new_name = &(new_file.as_ref()).or(old_file.as_ref()).unwrap().path;
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
                    state.emit(old_file.path.as_ref());
                    state.emit(b"\n");
                    state.emit(b"rename to ");
                    state.emit(new_file.path.as_ref());
                    state.emit(b"\n");
                }
                CopyInfo::Copy => {
                    state.emit(b"copy from ");
                    state.emit(old_file.path.as_ref());
                    state.emit(b"\n");
                    state.emit(b"copy to ");
                    state.emit(new_file.path.as_ref());
                    state.emit(b"\n");
                }
                CopyInfo::None => {}
            }
        }
        // Impossible here.
        (None, None) => {}
    }

    // When the files have no differences we shouldn't print any further
    // headers - those are reserved for changed files.
    if let (Some(old_file), Some(new_file)) = (&old_file, &new_file) {
        if old_file.contents == new_file.contents {
            return state.collect();
        }
    }

    // Header for binary files
    if file_is_binary(&old_file)
        || file_is_binary(&new_file)
        || file_is_omitted(&old_file)
        || file_is_omitted(&new_file)
    {
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
            _ => {}
        }
        return state.collect();
    }

    // Don't print anything else when adding an empty file.
    if old_file.is_none() && file_is_empty(&new_file) {
        return state.collect();
    }

    // Don't print anything else when removing an empty file.
    if file_is_empty(&old_file) && new_file.is_none() {
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

    if file_is_submodule(&old_file) || file_is_submodule(&new_file) {
        state.emit_submodule_changes(
            old_file
                .as_ref()
                .and_then(|file| file.contents.submodule_commit_hash()),
            new_file
                .as_ref()
                .and_then(|file| file.contents.submodule_commit_hash()),
        );
        return state.collect();
    }

    // All headers emitted, now emit the actual diff.
    let (reduce, seed) = state.unwrap();
    match (
        &old_file.map(|file| file.contents),
        &new_file.map(|file| file.contents),
    ) {
        (Some(FileContent::Inline(old_file)), Some(FileContent::Inline(new_file))) => {
            // Typical case, we need to call actual diff function to get the diff.
            let opts = HeaderlessDiffOpts {
                context: diff_opts.context,
            };
            gen_diff_unified_headerless(old_file, new_file, opts, seed, reduce)
        }
        (Some(FileContent::Inline(old_file)), None) => {
            // Degenerated case of all-minus diff.
            gen_diff_unified_headerless_entire_file(b'-', old_file, seed, reduce)
        }
        (None, Some(FileContent::Inline(new_file))) => {
            // Degenerated case of all-plus diff.
            gen_diff_unified_headerless_entire_file(b'+', new_file, seed, reduce)
        }
        _ => {
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
    C: AsRef<[u8]> + PartialEq + Eq,
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
    fn test_diff_hunks() {
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
                    contents: FileContent::Inline(&a),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                Some(DiffFile {
                    contents: FileContent::Inline(&b),
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
    fn test_diff_unified_file_removal() {
        let a = r#"a
b
c
d
"#;
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: FileContent::Inline(&a),
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
                    contents: FileContent::Inline(&""),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                Some(DiffFile {
                    contents: FileContent::Inline(&a),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
--- a/x
+++ b/x
@@ -0,0 +1,4 @@
+a
+b
+c
+d
"
        );
    }

    #[test]
    fn test_diff_unified_with_zero_context_lines() {
        let a = r#"lorem
ipsum
dolor
sit
consectetur
"#;

        let b = r#"lorem
dolor
sit
amet
consectetur
"#;

        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: FileContent::Inline(&a),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                Some(DiffFile {
                    contents: FileContent::Inline(&b),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                DiffOpts {
                    context: 0,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
--- a/x
+++ b/x
@@ -2,1 +1,0 @@
-ipsum
@@ -4,0 +4,1 @@
+amet
"
        );
    }

    #[test]
    fn test_diff_unified_with_large() {
        let hash1 = "hash1".to_string();
        let content_type_first: FileContent<Vec<u8>> = FileContent::Omitted {
            content_hash: hash1,
        };
        let hash2 = "hash2".to_string();
        let content_type_second: FileContent<Vec<u8>> = FileContent::Omitted {
            content_hash: hash2,
        };
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: content_type_first,
                    path: "x",
                    file_type: FileType::Regular,
                }),
                Some(DiffFile {
                    contents: content_type_second,
                    path: "x",
                    file_type: FileType::Regular,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
Binary file x has changed
"
        );
    }

    #[test]
    fn test_diff_unified_adding_empty_file() {
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                None,
                Some(DiffFile {
                    contents: FileContent::Inline(&""),
                    path: "x",
                    file_type: FileType::Regular,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
new file mode 100644
"
        );
    }

    #[test]
    fn test_diff_unified_removing_empty_file() {
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: FileContent::Inline(&""),
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
"
        );
    }

    #[test]
    fn test_blocks() {
        assert_eq!(
            blocks(b"a\nb\nc\nd\nx\ny\nz\n", b"b\nc\nd\ne\nf\nu\nv\nw\nx\n"),
            [(1, 4, 0, 3), (4, 5, 8, 9), (7, 7, 9, 9)],
        );
    }

    #[test]
    fn test_edit_cost_small_diffs() {
        let c = |a: &str, b: &str, max_edit_cost: u64| {
            // Insert "\n" per character.
            let a = a
                .chars()
                .map(|c| format!("{c}\n"))
                .collect::<Vec<_>>()
                .concat();
            let b = b
                .chars()
                .map(|c| format!("{c}\n"))
                .collect::<Vec<_>>()
                .concat();
            edit_cost(a.as_bytes(), b.as_bytes(), max_edit_cost)
        };

        // Same content.
        assert_eq!(c("", "", 0), 0);
        assert_eq!(c("", "", 10), 0);
        assert_eq!(c("xyz", "xyz", 0), 0);
        assert_eq!(c("xyz", "xyz", 10), 0);

        // Unique lines.
        assert_eq!(c("", "xyz", 10), 3);
        assert_eq!(c("abcd", "", 10), 4);
        assert_eq!(c("abcd", "xyz", 10), 7);

        // Capped unique lines.
        assert_eq!(c("abcd", "xyz", 6), 6);
        assert_eq!(c("abcd", "xyz", 5), 5);

        // "One side empty" fast path.
        assert_eq!(c("ab", "abb", 10), 1);
        assert_eq!(c("ab", "aabb", 10), 2);
        assert_eq!(c("aabb", "ab", 10), 2);
        assert_eq!(c("aab", "ab", 10), 1);

        // No fast path - run xdl_split.
        assert_eq!(c("abc", "cba", 10), 4);
        assert_eq!(c("abcd", "dcba", 10), 6);
        assert_eq!(c("abcdefg", "gfedcba", 3), 3);

        // Mixed. xdl_split and unique lines.
        assert_eq!(c("abcxyz", "cba", 10), 7);
        assert_eq!(c("abcxyz", "cba", 4), 4);
    }

    #[test]
    fn test_edit_cost_large_diff() {
        // Large diff. Return early set by max_edit_cost.
        // Avoid allocation when generating the test case.
        // This test case is generally challenging for diff algorithms.
        let n = 10_000_000;
        let mut a: Vec<u8> = Vec::with_capacity(n);
        let mut b: Vec<u8> = Vec::with_capacity(n);
        a.resize(n, b'\n');
        b.resize(n, b'\n');
        // Range of printable ASCII characters.
        let start_byte = b' ';
        let end_byte = b'~';
        let mut a_byte = start_byte;
        let mut b_byte = end_byte;
        for i in (0..n).step_by(2) {
            a[i] = a_byte;
            b[i] = b_byte;
            a_byte += 1;
            b_byte -= 1;
            if a_byte > end_byte {
                a_byte = start_byte;
                b_byte = end_byte;
            }
        }

        // Uncomment to write the test case to disk.
        // std::fs::write("a", &a).unwrap();
        // std::fs::write("b", &b).unwrap();

        // This can complete relatively fast (run with --release to reduce test generation time).
        // When n = 10M, edit_cost takes ~0.6s, full xdiff (blocks(), or git diff --stat) takes ~68s.
        assert_eq!(edit_cost(&a, &b, 100), 100);
    }

    #[test]
    fn test_edit_cost_against_diff_hunks() {
        // Edit cost should match diff_hunks line count.
        const CHARS: [u8; 4] = [b'x', b'y', b'z', b'\n'];
        let to_slice = |bits: u8| -> [u8; 8] {
            [
                CHARS[(bits & 3) as usize],
                b'\n',
                CHARS[((bits >> 2) & 3) as usize],
                b'\n',
                CHARS[((bits >> 4) & 3) as usize],
                b'\n',
                CHARS[((bits >> 6) & 3) as usize],
                b'\n',
            ]
        };
        (0..=0xffu8).for_each(|i| {
            let a = to_slice(i);
            (0..=0xffu8).for_each(|i| {
                let b = to_slice(i);
                let cost1 = edit_cost(&a, &b, 1000);
                let cost2 = diff_hunks(&a, &b)
                    .into_iter()
                    .fold(0, |a, h| a + h.add.len() + h.remove.len())
                    as u64;
                use std::str::from_utf8;
                assert_eq!(
                    cost1,
                    cost2,
                    "edit cost does not match: {:?} {:?}",
                    from_utf8(&a).unwrap(),
                    from_utf8(&b).unwrap()
                );
            });
        });
    }

    #[test]
    fn test_diff_unified_submodule_add() {
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                None,
                Some(DiffFile {
                    contents: FileContent::<Vec<u8>>::Submodule {
                        commit_hash: String::from("abcdef")
                    },
                    path: "x",
                    file_type: FileType::GitSubmodule,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
new file mode 160000
--- /dev/null
+++ b/x
@@ -0,0 +1 @@
+Subproject commit abcdef
"
        );
    }

    #[test]
    fn test_diff_unified_submodule_change() {
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: FileContent::<Vec<u8>>::Submodule {
                        commit_hash: String::from("abcdef1000")
                    },
                    path: "x",
                    file_type: FileType::GitSubmodule,
                }),
                Some(DiffFile {
                    contents: FileContent::Submodule {
                        commit_hash: String::from("abcdef2000")
                    },
                    path: "x",
                    file_type: FileType::GitSubmodule,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
--- a/x
+++ b/x
@@ -1 +1 @@
-Subproject commit abcdef1000
+Subproject commit abcdef2000
"
        );
    }

    #[test]
    fn test_diff_unified_submodule_replace() {
        let a = "a\n";
        assert_eq!(
            String::from_utf8_lossy(&diff_unified(
                Some(DiffFile {
                    contents: FileContent::Submodule {
                        commit_hash: String::from("abcdef")
                    },
                    path: "x",
                    file_type: FileType::GitSubmodule,
                }),
                Some(DiffFile {
                    contents: FileContent::Inline(&a),
                    path: "x",
                    file_type: FileType::Executable,
                }),
                DiffOpts {
                    context: 10,
                    copy_info: CopyInfo::None,
                }
            )),
            r"diff --git a/x b/x
deleted file mode 160000
--- a/x
+++ /dev/null
@@ -1 +0,0 @@
-Subproject commit abcdef
diff --git a/x b/x
new file mode 100755
--- /dev/null
+++ b/x
@@ -0,0 +1,1 @@
+a
"
        );
    }
}
