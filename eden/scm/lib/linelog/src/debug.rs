/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;

use crate::linelog::AbstractLineLog;
use crate::linelog::Inst;

impl<T: fmt::Display> AbstractLineLog<T> {
    /// Dump instructions in a human readable format. Useful for debugging.
    /// Note: This exposes internal details which might change in the future.
    pub fn describe_instructions(&self) -> Vec<String> {
        self.code
            .iter()
            .enumerate()
            .map(|(i, inst)| format!("{i}: {}", describe_inst(inst)))
            .collect()
    }
}

fn describe_inst<T: fmt::Display>(inst: &Inst<T>) -> String {
    match inst {
        Inst::J(pc) => format!("J {pc}"),
        Inst::JGE(rev, pc) => format!("JGE {rev} {pc}"),
        Inst::JL(rev, pc) => format!("JL {rev} {pc}"),
        Inst::LINE(rev, data) => {
            let trimmed = data.to_string();
            let trimmed = trimmed.trim_end();
            format!("LINE {rev} {trimmed:?}")
        }
        Inst::END => "END".to_string(),
    }
}
