/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Handles nested padding in `Debug::fmt` output.
//!
//! `PadAdapter` is taken from rustc:
//! https://github.com/rust-lang/rust/commit/e3656bd81baa3c2cb5065da04f9debf378f99772

use std::fmt;

struct PadAdapter<'a, 'b: 'a> {
    fmt: &'a mut fmt::Formatter<'b>,
    on_newline: bool,
}

impl<'a, 'b: 'a> PadAdapter<'a, 'b> {
    fn new(fmt: &'a mut fmt::Formatter<'b>) -> PadAdapter<'a, 'b> {
        PadAdapter {
            fmt,
            on_newline: false,
        }
    }
}

impl<'a, 'b: 'a> fmt::Write for PadAdapter<'a, 'b> {
    fn write_str(&mut self, mut s: &str) -> fmt::Result {
        while !s.is_empty() {
            if self.on_newline {
                self.fmt.write_str("  ")?;
            }

            let split = match s.find('\n') {
                Some(pos) => {
                    self.on_newline = true;
                    pos + 1
                }
                None => {
                    self.on_newline = false;
                    s.len()
                }
            };
            self.fmt.write_str(&s[..split])?;
            s = &s[split..];
        }

        Ok(())
    }
}

/// Write Debug output with alternative form considered.
pub fn write_debug(formatter: &mut fmt::Formatter, value: impl fmt::Debug) -> fmt::Result {
    if formatter.alternate() {
        let mut writer = PadAdapter::new(formatter);
        fmt::write(&mut writer, format_args!("\n{:#?}", value))
    } else {
        write!(formatter, " {:?}", value)
    }
}
