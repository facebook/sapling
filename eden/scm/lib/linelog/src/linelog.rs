/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use bitflags::bitflags;
use im::Vector as ImVec;

use crate::maybe_mut::MaybeMut;
use crate::nanodag::NanoDag;

/// See https://sapling-scm.com/docs/internals/linelog for details.
pub struct AbstractLineLog<T> {
    pub(crate) code: ImVec<Inst<T>>,
    pub(crate) max_rev: Rev,
    pub(crate) dag: NanoDag,

    a_lines_cache: Option<(Rev, ImVec<LineInfo<T>>)>,
    deps_map_cache: OnceLock<Arc<NanoDag>>,
    perf_stats: Option<Arc<PerfStats>>,
}

/// Performance statistics. Useful in tests.
#[derive(Default, Debug)]
pub(crate) struct PerfStats {
    /// How many times the a_lines_cache gets hit.
    pub cache_hit: AtomicUsize,
    /// How many times execute() is called.
    pub execute: AtomicUsize,
    /// How many times the dag cache gets initialized.
    pub dag_cache: AtomicUsize,
}

impl<T> Clone for AbstractLineLog<T> {
    fn clone(&self) -> Self {
        Self {
            code: self.code.clone(),
            max_rev: self.max_rev,
            dag: self.dag.clone(),
            a_lines_cache: self.a_lines_cache.clone(),
            deps_map_cache: self.deps_map_cache.clone(),
            perf_stats: self.perf_stats.clone(),
        }
    }
}

/// Information about a line.
#[cfg_attr(debug_assertions, derive(Debug, PartialEq))]
pub struct LineInfo<T> {
    /// Line content.
    pub data: Arc<T>,
    /// Introduced rev.
    pub rev: Rev,
    /// Introduced instruction index.
    pub pc: Pc,
    /// Whether the line was deleted or not.
    pub deleted: bool,
}

impl<T> Clone for LineInfo<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            rev: self.rev,
            pc: self.pc,
            deleted: self.deleted,
        }
    }
}

pub(crate) type Pc = usize;
pub(crate) type Rev = usize;
type LineIdx = usize;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug)]
pub(crate) enum Inst<T> {
    J(Pc),
    END,
    JGE(Rev, Pc),
    JL(Rev, Pc),
    LINE(Rev, Arc<T>),
}

impl<T> Clone for Inst<T> {
    fn clone(&self) -> Self {
        match self {
            Self::J(pc) => Self::J(*pc),
            Self::END => Self::END,
            Self::JGE(rev, pc) => Self::JGE(*rev, *pc),
            Self::JL(rev, pc) => Self::JL(*rev, *pc),
            Self::LINE(rev, line) => Self::LINE(*rev, line.clone()),
        }
    }
}

impl<T> Default for AbstractLineLog<T> {
    fn default() -> Self {
        Self {
            code: {
                let mut v = ImVec::new();
                v.push_back(Inst::END);
                v
            },
            max_rev: 0,
            dag: Default::default(),
            a_lines_cache: None,
            deps_map_cache: OnceLock::new(),
            perf_stats: None,
        }
    }
}

impl<T> AbstractLineLog<T> {
    /// Get the maximum rev (inclusive).
    pub fn max_rev(&self) -> Rev {
        self.max_rev
    }

    /// Attach a `PerfStats` struct to analyse cache statistics.
    pub(crate) fn with_perf_stats(self, stats: Option<Arc<PerfStats>>) -> Self {
        let dag = self.dag.with_perf_stats(stats.clone());
        Self {
            perf_stats: stats,
            dag,
            ..self
        }
    }
}

bitflags! {
    #[derive(Clone, Copy)]
    pub struct EditFlags: u32 {
        /// When inserting a block, try to shift it around to relax dependency.
        const BLOCK_SHIFT = 0b00000001;
        /// When editing rev_a -> rev_b, try to add an edge in the dag.
        const ADD_EDGE = 0b00000010;
    }
}

impl Default for EditFlags {
    fn default() -> Self {
        Self::BLOCK_SHIFT | Self::ADD_EDGE
    }
}

impl<T: Default + PartialEq + fmt::Debug> AbstractLineLog<T> {
    /// Edit chunk. Replace lines from `a1` (inclusive) to `a2` (exclusive) in rev
    /// `a_rev` with `b_lines`. `b_lines` are considered introduced by `b_rev`.
    /// If `b_lines` is empty, the edit is a deletion. If `a1` equals to `a2`,
    /// the edit is an insertion. Otherwise, the edit is a modification.
    ///
    /// While this function does not cause conflicts or error out, not all
    /// editings make practical sense. The callsite might want to do some
    /// extra checks to ensure the edit is meaningful.
    ///
    /// If `BLOCK_SHIFT` flag is set (default), consider shifting the insertion
    /// lines to relax dependency for easier reordering. Check the comments of
    /// `try_block_shift` for details. Block shift requires that `T::eq(l1, l2)`
    /// means `l1` and `l2` have the same content. If this cannot be guaranteed,
    /// disable `BLOCK_SHIFT`.
    ///
    /// If `ADD_EDGE` flag is set (default), also add an edge in the dag to
    /// suggest `a_rev` is a parent of `b_rev` when `a_rev < b_rev`.
    ///
    /// Panics if `a_rev` > `max_rev`.
    pub fn edit_chunk(
        self,
        a_rev: Rev,
        mut a1: LineIdx,
        mut a2: LineIdx,
        b_rev: Rev,
        b_lines: Vec<T>,
        flags: EditFlags,
    ) -> Self {
        assert!(
            a_rev <= self.max_rev,
            "a_rev {a_rev} must not be greater than max_rev {}",
            self.max_rev
        );
        let this = if flags.contains(EditFlags::ADD_EDGE) && a_rev <= b_rev {
            Self {
                dag: self.dag.with_edge(a_rev, b_rev),
                ..self
            }
        } else {
            self
        };
        let mut b_lines = b_lines.into_iter().map(Arc::new).collect::<VecDeque<_>>();
        this.with_a_lines_cache(a_rev, b_rev, |this: Self, maybe_mut| {
            if flags.contains(EditFlags::BLOCK_SHIFT) {
                const DEFAULT_SHIFT_THRESHOLD: usize = 5;
                this.try_block_shift(
                    &maybe_mut,
                    &mut a1,
                    &mut a2,
                    &mut b_lines,
                    DEFAULT_SHIFT_THRESHOLD,
                );
            };
            this.edit_chunk_internal(a1, a2, b_rev, b_lines, maybe_mut)
        })
    }

    /// Add an edge in the dag without changing the linelog instructions.
    /// See [`NanoDag::with_edge`].
    /// Panics if `a_rev` > `b_rev`.
    /// Bumps `max_rev` if `b_rev` > `max_rev`.
    pub fn with_dag_edge(self, a_rev: Rev, b_rev: Rev) -> Self {
        let mut a_lines_cache = self.a_lines_cache;
        let new_dag = self.dag.with_edge(a_rev, b_rev);
        if let Some((cached_rev, _)) = &a_lines_cache {
            if new_dag.is_ancestor(b_rev, *cached_rev) {
                // The new edge affects content. Invalidates a_lines_cache.
                a_lines_cache = None;
            }
        }
        Self {
            dag: new_dag,
            max_rev: self.max_rev.max(b_rev),
            a_lines_cache,
            deps_map_cache: Default::default(),
            ..self
        }
    }

    /// Attempt to shift the insertion chunk so the start of insertion aligns
    /// with another "start insertion". This might trigger the [OPT1]
    /// optimization in `edit_chunk_internal`, avoid nested insertions and
    /// enable more flexible reordering.
    ///
    /// For example, we might get "Insert (rev 3)" below that forces a nested
    /// insertion block. However, if we shift the block and use the
    /// "Alternative Insert (rev 3)", we can use the [OPT1] optimization.
    ///
    /// ```text
    ///   +----Insert (rev 1)
    ///   |    Line:  function a () {
    ///   |    Line:    return 'a';
    ///   |    Line:  }
    ///   +----
    ///   +----Insert (rev 2)
    ///   |                           ----+ Alternative Insert (rev 3)
    ///   |    Line:                      |
    ///   |+---Insert (rev 3)             |
    ///   ||   Line:  function b () {     |
    ///   ||   Line:    return 'b';       |
    ///   ||   Line:  }                   |
    ///   ||                          ----+
    ///   ||   Line:
    ///   |+---
    ///   |    Line:  function c () {
    ///   |    Line:    return 'c';
    ///   |    Line:  }
    ///   +----
    /// ```
    ///
    /// Block shifting works if the surrounding lines match, see:
    ///
    /// ```text
    ///     A                                    A
    ///     B                                  +-------+
    ///   +-------+     is equivalent to       | B     |
    ///   | block |     === shift up   ==>     | block |
    ///   | B     |     <== shift down ===     +-------+
    ///   +-------+                              B
    ///     C                                    C
    /// ```
    ///
    /// Updates `a1`, `a2`, `b_lines` in-place if a better position was found,
    /// Does nothing if [OPT1] already applies or no shift helps.
    ///
    /// `threshold` decides the search range. O(threshold) complexity.
    fn try_block_shift(
        &self,
        a_lines: &ImVec<LineInfo<T>>,
        a1: &mut LineIdx,
        a2: &mut LineIdx,
        b_lines: &mut VecDeque<Arc<T>>,
        threshold: usize,
    ) {
        if *a1 != *a2 || b_lines.is_empty() {
            // Not an insertion. Skip.
            return;
        }

        let can_use_opt1 = |a: usize| -> bool {
            let Some(info) = a_lines.get(a) else {
                return false;
            };
            info.pc > 0 && matches!(self.code.get(info.pc - 1), Some(Inst::JL(..)))
        };

        if can_use_opt1(*a1) {
            return;
        }

        let mut consider_shift = |step: i32| -> Option<()> {
            let mut ai = *a1;
            let blen = b_lines.len();
            for k in 1..=threshold {
                if step < 0 && ai == 0 || step > 0 && ai == a_lines.len() - 1 {
                    return None;
                }
                // After k shifts, logical index i maps to b_lines[(i + k) % blen]
                // (shift down) or b_lines[(i + blen - k) % blen] (shift up).
                // Shift up: compare a_lines[ai-1] with logical last
                // Shift down: compare a_lines[ai] with logical first
                let (a_idx, b_phys) = if step < 0 {
                    (ai - 1, (blen - (k % blen)) % blen)
                } else {
                    (ai, (k - 1) % blen)
                };
                let a_data = &*a_lines.get(a_idx)?.data;
                let b_data = b_lines.get(b_phys)?;
                if a_data != b_data.as_ref() {
                    return None;
                }
                if step < 0 {
                    ai -= 1;
                } else {
                    ai += 1;
                }
                if can_use_opt1(ai) {
                    let rotate = if step < 0 { blen - (k % blen) } else { k };
                    b_lines.rotate_left(rotate % blen);
                    *a1 = ai;
                    *a2 = ai;
                    return Some(());
                }
            }
            None
        };

        let _ = consider_shift(-1).is_some() || consider_shift(1).is_some();
    }

    /// Checkout the lines of the given revision `rev`.
    pub fn checkout_lines(&self, rev: Rev) -> ImVec<LineInfo<T>> {
        if let Some((a_rev, cache)) = self.a_lines_cache.as_ref() {
            if *a_rev == rev {
                if let Some(stats) = self.perf_stats.as_ref() {
                    stats.cache_hit.fetch_add(1, Ordering::Release);
                }
                return cache.clone();
            }
        }

        self.execute(rev, rev, None)
    }

    /// Checkout the lines of the given revision range `start` to `end`, both
    /// inclusive.
    ///
    /// For example, if `start` is 0, and `rev` is `max_rev()`, the result will
    /// include all lines ever existed in all revisions.
    pub fn checkout_range_lines(&self, start: Rev, end: Rev) -> ImVec<LineInfo<T>> {
        let lines = self.checkout_lines(end);
        let present_pc_set = lines.into_iter().map(|l| l.pc).collect::<HashSet<Pc>>();
        let is_present = move |pc| present_pc_set.contains(&pc);
        self.execute(start, end, Some(Box::new(is_present)))
    }

    /// Prepare and update a_lines_cache for edit_chunk_internal
    /// (editing from a_rev to b_rev).
    /// - Prepare `a_lines` (checkout a_rev), reuse `self.a_lines_cache`
    ///   if possible (e.g. a_rev matches).
    /// - Call `func` (edit function) with the a_lines.
    /// - `func` can edit `a_lines` with the intention that the updated
    ///   `a_lines` becomes `b_lines` and the `(b_rev, b_lines)` can be
    ///   put back to `self.a_lines_cache`.
    /// - Update `self.a_lines_cache` accordingly, if possible.
    ///
    /// This makes sequential edits, like (a_rev=1, b_rev=2),
    /// (a_rev=2, b_rev=2), (a_rev=2, b_rev=3), (a_rev=3, b_rev=4), ...
    /// hit the cache.
    fn with_a_lines_cache(
        mut self,
        a_rev: Rev,
        b_rev: Rev,
        func: impl FnOnce(Self, MaybeMut<ImVec<LineInfo<T>>>) -> Self,
    ) -> Self {
        let cache = self.a_lines_cache.take();

        // Reuse or rebuild cache.
        let mut a_lines: ImVec<LineInfo<T>> = (match cache {
            Some((rev, a_lines)) if rev == a_rev => {
                if let Some(stats) = self.perf_stats.as_ref() {
                    stats.cache_hit.fetch_add(1, Ordering::Release);
                }
                Some(a_lines)
            }
            _ => None,
        })
        .unwrap_or_else(|| self.execute(a_rev, a_rev, None));

        // Can only update cache if there are no possible edits between a_rev and b_rev.
        // It could be a_rev == b_rev, or b_rev >= a_rev >= max_rev.
        let can_update_cache = a_rev == b_rev || (b_rev >= a_rev && a_rev >= self.max_rev);
        let maybe_a_lines: MaybeMut<_> = match can_update_cache {
            true => MaybeMut::Mut(&mut a_lines),
            false => MaybeMut::Ref(&a_lines),
        };

        let mut result = func(self, maybe_a_lines);

        // Maybe update cache.
        if can_update_cache {
            #[cfg(debug_assertions)]
            {
                let fresh_lines = result
                    .clone()
                    .with_perf_stats(None)
                    .execute(b_rev, b_rev, None);
                assert_eq!(fresh_lines, a_lines);
            }
            result.a_lines_cache = Some((b_rev, a_lines));
        } else {
            result.a_lines_cache = None;
        }

        result
    }

    // private because of `a_lines`.
    fn edit_chunk_internal(
        self,
        a1: LineIdx,
        a2: LineIdx,
        b_rev: Rev,
        b_lines: VecDeque<Arc<T>>,
        mut a_lines: MaybeMut<ImVec<LineInfo<T>>>,
    ) -> Self {
        assert!(a1 <= a2);
        assert!(a2 <= a_lines.len());

        if a1 == a2 && b_lines.is_empty() {
            return Self {
                max_rev: self.max_rev.max(b_rev),
                dag: self.dag.with_edge(b_rev, b_rev),
                ..self
            };
        }

        let start = self.code.len();

        // See also https://sapling-scm.com/docs/internals/linelog/#editing-linelog
        //
        // Ported from
        // https://github.com/facebook/sapling/blob/9f55ce6e/addons/isl/src/linelog.ts
        //
        // # Before             # After
        // # (pc): Instruction  # (pc): Instruction
        //       : ...                : ...
        //   a1Pc: <a1Inst>      a1Pc: J start
        // a1Pc+1: ...          a1Pc+1: ...
        //       : ...                : ...
        //   a2Pc: ...            a2Pc: ...
        //       : ...                : ...
        //    len: N/A           start: JL brev b2Pc      [1]
        //                            : LINE brev b1      [1]
        //                            : LINE brev b1+1    [1]
        //                            : ...               [1]
        //                            : LINE brev b2-1    [1]
        //                        b2Pc: JGE brev a2Pc     [2]
        //                            : <a1Inst> (moved)  [3]
        //                            : J a1Pc+1          [4]
        // [1]: Only present if `bLines` is not empty.
        // [2]: Only present if `a1 < a2`.
        //      There are 2 choices for "a2Pc":
        //      - The a2 line exactly: aLines[a2].pc
        //      - The next instruction of the "a2 -1" line: aLines[a2 - 1].pc + 1
        //      We pick the latter to avoid overly aggressive deletion.
        //      The original C implementation might pick the former when editing
        //      the last rev for performance optimization.
        // [3]: <a1Inst> could be LINE or END.
        // [4]: As an optimization, this is only present if <a1Inst> is not END.
        //
        // Optimization [OPT1] to make reorder less restrictive, treat insertion
        // (a1 == a2) at the beginning of another insertion (<a1Inst> is after a
        // <JL>) specially. Our goal is to avoid nested JLs. Instead of patching
        // the a1Inst after the JL, we patch the JL (jlInst) so we can insert our
        // new JL (for this edit) before the old JL (jlInst, being patched).
        // Note this "JL followed by a1Inst" optimization needs to be applicable
        // multiple times. To do that, we also move the a1Inst to right after the
        // jlInst so the pattern "JL followed by a1Inst" can be recognized by the
        // next editChunk to apply the same optimization.
        //
        // # Before             # After
        // # (pc): Instruction  # (pc): Instruction
        //       : ...                : ...
        //       : <jlInst>     a1Pc-1: J start           [*]
        //   a1Pc: <a1Inst>       a1Pc: NOP (J a1Pc+1)    [*]
        //       : ...                : ...
        //    len: N/A           start: JL brev b2Pc
        //                            : (bLines)
        //                        b2Pc: <jlInst> (moved)  [*]
        //                            : <a1Inst> (moved)
        //                            : J a1Pc            [*]

        // Prepare updating a_lines.
        let new_b_lines = if a_lines.is_mut() {
            b_lines
                .iter()
                .enumerate()
                .map(|(i, line)| LineInfo {
                    data: line.clone(),
                    rev: b_rev,
                    pc: start + i + 1,
                    deleted: false,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Update code.
        let mut code = self.code;
        let a1_pc = a_lines[a1].pc;
        // If `jl_inst` is set, optimization [OPT1] is in effect.
        let mut jl_inst = if a1_pc > 0 && a1 == a2 {
            code.get(a1_pc - 1).cloned()
        } else {
            None
        };
        if !matches!(jl_inst, Some(Inst::JL(..))) {
            jl_inst = None;
        }
        if !b_lines.is_empty() {
            // [1]
            let b2_pc = start + b_lines.len() + 1;
            code.push_back(Inst::JL(b_rev, b2_pc));
            for line in b_lines {
                code.push_back(Inst::LINE(b_rev, line));
            }
            debug_assert_eq!(b2_pc, code.len());
        }
        if a1 < a2 {
            debug_assert!(jl_inst.is_none(), "OPT1 requires no deletion");
            // [2]
            let a2_pc = a_lines[a2 - 1].pc + 1;
            code.push_back(Inst::JGE(b_rev, a2_pc));
        }
        if let Some(a_lines_mut) = a_lines.get_mut() {
            a_lines_mut[a1].pc = if jl_inst.is_none() {
                code.len()
            } else {
                code.len() + 1
            };
        }
        if let Some(jl_inst) = jl_inst {
            // [OPT1] Move jlInst and a1Inst, NOP original a1Pc.
            let a1_inst = code[a1_pc].clone();
            code.push_back(jl_inst);
            code.push_back(a1_inst);
            code.push_back(Inst::J(a1_pc));
            code.set(a1_pc - 1, Inst::J(start));
            code.set(a1_pc, Inst::J(a1_pc + 1));
        } else {
            // [3]
            let a1_inst = code[a1_pc].clone();
            let is_end = matches!(a1_inst, Inst::END);
            code.push_back(a1_inst);
            if !is_end {
                // [4]
                code.push_back(Inst::J(a1_pc + 1));
            }
            code.set(a1_pc, Inst::J(start));
        }

        // Update a_lines.
        if let Some(a_lines_mut) = a_lines.get_mut() {
            // slice removes a1..a2
            let mut new_a_lines = a_lines_mut.take(a1);
            new_a_lines.extend(new_b_lines);
            new_a_lines.append(a_lines_mut.slice(a2..));
            *a_lines_mut = new_a_lines;
        }

        Self {
            code,
            max_rev: self.max_rev.max(b_rev),
            a_lines_cache: None,
            deps_map_cache: Default::default(),
            ..self
        }
    }

    // private because of `present`. no caching.
    fn execute(
        &self,
        start_rev: Rev,
        end_rev: Rev,
        present: Option<Box<dyn Fn(Pc) -> bool>>,
    ) -> ImVec<LineInfo<T>> {
        if let Some(stats) = self.perf_stats.as_ref() {
            stats.execute.fetch_add(1, Ordering::Release);
        }
        let mut lines = ImVec::<LineInfo<T>>::new();
        let mut pc = 0;
        // Each instructions should be executed at most once. There is no loop.
        let mut patience = self.code.len() + 1;
        let is_deleted = |pc: Pc| match present.as_deref() {
            Some(present) => !present(pc),
            None => false,
        };
        while patience > 0 {
            let code = &self.code[pc];
            match code {
                Inst::J(j_pc) => {
                    pc = *j_pc;
                }
                Inst::END => {
                    lines.push_back(LineInfo {
                        data: Arc::new(Default::default()),
                        rev: 0,
                        deleted: true,
                        pc,
                    });
                    break;
                }
                Inst::JGE(rev, j_pc) => {
                    if self.dag.is_ancestor(*rev, start_rev) {
                        pc = *j_pc;
                    } else {
                        pc += 1;
                    }
                }
                Inst::JL(rev, j_pc) => {
                    if !self.dag.is_ancestor(*rev, end_rev) {
                        pc = *j_pc;
                    } else {
                        pc += 1;
                    }
                }
                Inst::LINE(rev, data) => {
                    lines.push_back(LineInfo {
                        data: data.clone(),
                        rev: *rev,
                        deleted: is_deleted(pc),
                        pc,
                    });
                    pc += 1;
                }
            }
            patience -= 1;
        }
        assert!(patience > 0, "bug: code does not terminate");
        lines
    }
}

impl<T> AbstractLineLog<T> {
    /// Rewrite `rev` references in all instructions.
    /// This can be useful for reordering, folding, or inserting revisions.
    ///
    /// The edges in the DAG are *unchanged*. If larger revs are introduced,
    /// the dag will resize to make sure all revs are present in the dag.
    ///
    /// Note: There are no checks about whether the reordering is meaningful.
    /// The callsite should use `can_reorder` to check dependencies
    /// and avoid troublesome reorders, like moving a change to before its
    /// dependency.
    pub fn remap_revs(self, rev_map: &dyn Fn(Rev) -> Rev) -> Self {
        let mut max_rev = 0;
        let code = self
            .code
            .into_iter()
            .map(|inst| {
                let mapped = match inst {
                    Inst::JGE(rev, pc) => Inst::JGE(rev_map(rev), pc),
                    Inst::JL(rev, pc) => Inst::JL(rev_map(rev), pc),
                    Inst::LINE(rev, data) => Inst::LINE(rev_map(rev), data),
                    other => other,
                };
                if let Inst::JGE(rev, _) | Inst::JL(rev, _) | Inst::LINE(rev, _) = &mapped {
                    max_rev = max_rev.max(*rev);
                }
                mapped
            })
            .collect();

        Self {
            code,
            max_rev,
            dag: self.dag.with_edge(max_rev, max_rev),
            a_lines_cache: None,
            deps_map_cache: Default::default(),
            ..self
        }
    }

    /// Insert a rev after `rev`.
    ///
    /// Original `r` (`r > rev`) will shift to `r + 1` in both the linelog
    /// instructions and the dag.
    pub fn insert_shift(self, rev: Rev) -> Self {
        let result = self.remap_revs(&|r| if r > rev { r + 1 } else { r });
        let dag = result.dag.insert_shift(rev);
        Self { dag, ..result }
    }

    /// Truncate linelog. Drop revs >= the given `rev`.
    pub fn truncate(self, rev: Rev) -> Self {
        let mut max_rev = 0;
        let code = self
            .code
            .into_iter()
            .enumerate()
            .map(|(pc, inst)| match inst {
                Inst::JGE(r, _) | Inst::LINE(r, _) if r >= rev => Inst::J(pc + 1),
                Inst::JL(r, target) if r >= rev => Inst::J(target),
                other => {
                    if let Inst::JGE(r, _) | Inst::JL(r, _) | Inst::LINE(r, _) = &other {
                        max_rev = max_rev.max(*r);
                    }
                    other
                }
            })
            .collect();

        Self {
            code,
            max_rev,
            a_lines_cache: None,
            deps_map_cache: Default::default(),
            ..self
        }
    }

    /// Access to the `nanodag`.
    pub fn nanodag(&self) -> &NanoDag {
        &self.dag
    }

    /// Get the dependency dag. If `rev1` has parent `rev2`, then `rev1`
    /// textually depend on `rev2` and cannot be moved to an ancestor of `rev2`.
    ///
    /// Only includes revs explicitly appear in linelog instructions, i.e. revs
    /// that actually edit the lines.
    pub fn dep_map(&self) -> &Arc<NanoDag> {
        self.deps_map_cache
            .get_or_init(|| Arc::new(self.calculate_dep_map()))
    }
}

impl<T: AsRef<str> + Default + PartialEq + fmt::Debug> AbstractLineLog<T> {
    /// Checkout the text of the given `rev`.
    pub fn checkout_text(&self, rev: Rev) -> String {
        let lines = self.checkout_lines(rev);
        let mut text =
            String::with_capacity(lines.iter().map(|l| l.data.as_ref().as_ref().len()).sum());
        for line in lines {
            text.push_str(line.data.as_ref().as_ref());
        }
        text
    }
}
