/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use im::Vector as ImVec;

use crate::maybe_mut::MaybeMut;

/// See https://sapling-scm.com/docs/internals/linelog for details.
#[derive(Clone)]
pub struct AbstractLineLog<T> {
    code: ImVec<Inst<T>>,
    max_rev: Rev,

    a_lines_cache: Option<(Rev, ImVec<LineInfo<T>>)>,
}

/// Information about a line.
#[derive(Clone)]
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

type Pc = usize;
type Rev = usize;
type LineIdx = usize;

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Debug)]
enum Inst<T> {
    J(Pc),
    END,
    JGE(Rev, Pc),
    JL(Rev, Pc),
    LINE(Rev, Arc<T>),
}

impl<T: Clone> Default for AbstractLineLog<T> {
    fn default() -> Self {
        Self {
            code: {
                let mut v = ImVec::new();
                v.push_back(Inst::END);
                v
            },
            max_rev: 0,
            a_lines_cache: None,
        }
    }
}

impl<T> AbstractLineLog<T> {
    /// Get the maximum rev (inclusive).
    pub fn max_rev(&self) -> Rev {
        self.max_rev
    }
}

impl<T: Clone + Default + PartialEq + fmt::Debug> AbstractLineLog<T> {
    /// Edit chunk. Replace lines from `a1` (inclusive) to `a2` (exclusive) in rev
    /// `a_rev` with `b_lines`. `b_lines` are considered introduced by `b_rev`.
    /// If `b_lines` is empty, the edit is a deletion. If `a1` equals to `a2`,
    /// the edit is an insertion. Otherwise, the edit is a modification.
    ///
    /// While this function does not cause conflicts or error out, not all
    /// editings make practical sense. The callsite might want to do some
    /// extra checks to ensure the edit is meaningful.
    pub fn edit_chunk(
        self,
        a_rev: Rev,
        a1: LineIdx,
        a2: LineIdx,
        b_rev: Rev,
        b_lines: Vec<T>,
    ) -> Self {
        self.with_a_lines_cache(a_rev, b_rev, |this: Self, maybe_mut| {
            this.edit_chunk_internal(a1, a2, b_rev, b_lines, maybe_mut)
        })
    }

    /// Checkout the lines of the given revision `rev`.
    pub fn checkout_lines(&self, rev: Rev) -> ImVec<LineInfo<T>> {
        if let Some((a_rev, cache)) = self.a_lines_cache.as_ref() {
            if *a_rev == rev {
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

    fn with_a_lines_cache(
        mut self,
        a_rev: Rev,
        b_rev: Rev,
        func: impl FnOnce(Self, MaybeMut<ImVec<LineInfo<T>>>) -> Self,
    ) -> Self {
        let cache = self.a_lines_cache.take();

        // Reuse or rebuild cache.
        let mut a_lines: ImVec<LineInfo<T>> = (match cache {
            Some((rev, a_lines)) if rev == a_rev || (rev == self.max_rev && a_rev > rev) => {
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
                let fresh_lines = result.execute(b_rev, b_rev, None);
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
        b_lines: Vec<T>,
        mut a_lines: MaybeMut<ImVec<LineInfo<T>>>,
    ) -> Self {
        assert!(a1 <= a2);
        assert!(a2 <= a_lines.len());

        if a1 == a2 && b_lines.is_empty() {
            return self;
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
        //     a1: <a1 Inst>      a1Pc: J start
        //   a1+1: ...          a1Pc+1: ...
        //       : ...                : ...
        //     a2: ...            a2Pc: ...
        //       : ...                : ...
        //    len: N/A           start: JL brev b2Pc      [1]
        //                            : LINE brev b1      [1]
        //                            : LINE brev b1+1    [1]
        //                            : ...               [1]
        //                            : LINE brev b2-1    [1]
        //                        b2Pc: JGE brev a2Pc     [2]
        //                            : <a1 Inst> (moved) [3]
        //                            : J a1Pc+1          [4]
        // [1]: Only present if `bLines` is not empty.
        // [2]: Only present if `a1 < a2`.
        //      There are 2 choices for "a2Pc":
        //      - The a2 line exactly: aLines[a2].pc
        //      - The next instruction of the "a2 -1" line: aLines[a2 - 1].pc + 1
        //      We pick the latter to avoid overly aggressive deletion.
        //      The original C implementation might pick the former when editing
        //      the last rev for performance optimization.
        // [3]: <a1 Inst> could be LINE or END.
        // [4]: As an optimization, this is only present if <a1 Inst> is not END.
        //
        // As an optimization to make reorder less restrictive, we treat insertion
        // (a1 == a2) at the beginning of another insertion (<a1 Inst> is after a
        // <JL>) specially by patching the <JL> instruction instead of <a1 Inst>
        // and make sure the new <JL> (for this edit) is before the old <JL>.
        // See the [*] lines below for differences with the above:
        //
        // # Before             # After
        // # (pc): Instruction  # (pc): Instruction
        //       : ...                : ...
        //       : <JL>         a1Pc-1: J start           [*]
        //     a1: <a1 Inst>      a1Pc: ... (unchanged)   [*]
        //       : ...                : ...
        //    len: N/A           start: JL brev b2Pc
        //                            : ...
        //                        b2Pc: <JL> (moved)      [*]
        //                            : J a1Pc            [*]

        // Prepare updating a_lines.
        let new_b_lines = if a_lines.is_mut() {
            b_lines
                .iter()
                .enumerate()
                .map(|(i, line)| LineInfo {
                    data: Arc::new(line.clone()),
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
        let mut jl_inst = if a1_pc > 0 && a1 == a2 {
            code.get(a1_pc - 1).cloned()
        } else {
            None
        };
        if !matches!(jl_inst, Some(Inst::JL(..))) {
            jl_inst = None;
        }
        if !b_lines.is_empty() {
            let b2_pc = start + b_lines.len() + 1;
            code.push_back(Inst::JL(b_rev, b2_pc));
            for line in b_lines {
                code.push_back(Inst::LINE(b_rev, Arc::new(line)));
            }
            debug_assert_eq!(b2_pc, code.len());
        }
        if a1 < a2 {
            debug_assert!(jl_inst.is_none(), "no deletions when jl_inst is present");
            let a2_pc = a_lines[a2 - 1].pc + 1;
            code.push_back(Inst::JGE(b_rev, a2_pc));
        }
        if let (Some(a_lines_mut), None) = (a_lines.get_mut(), &jl_inst) {
            a_lines_mut[a1].pc = code.len();
        }
        if let Some(jl_inst) = jl_inst {
            code.push_back(jl_inst);
            code.push_back(Inst::J(a1_pc));
            code.set(a1_pc - 1, Inst::J(start));
        } else {
            let a1_inst = code[a1_pc].clone();
            let is_end = matches!(a1_inst, Inst::END);
            code.push_back(a1_inst);
            if !is_end {
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
        }
    }

    // private because of `present`. no caching.
    fn execute(
        &self,
        start_rev: Rev,
        end_rev: Rev,
        present: Option<Box<dyn Fn(Pc) -> bool>>,
    ) -> ImVec<LineInfo<T>> {
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
                    if start_rev >= *rev {
                        pc = *j_pc;
                    } else {
                        pc += 1;
                    }
                }
                Inst::JL(rev, j_pc) => {
                    if end_rev < *rev {
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

impl AbstractLineLog<String> {
    /// Checkout the text of the given `rev`.
    pub fn checkout_text(&self, rev: Rev) -> String {
        let lines = self.checkout_lines(rev);
        let mut text = String::with_capacity(lines.iter().map(|l| l.data.len()).sum());
        for line in lines {
            text.push_str(line.data.as_ref());
        }
        text
    }
}
