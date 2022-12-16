/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

/// Instruction from parsed config.
#[derive(Clone, Debug)]
pub enum Instruction<'a> {
    /// Set a config.
    SetConfig {
        section: &'a str,
        name: &'a str,
        // For multi-line value, it can be an owned string.
        value: Cow<'a, str>,
        span: Range<usize>,
    },
    /// Unset a config.
    UnsetConfig {
        section: &'a str,
        name: &'a str,
        span: Range<usize>,
    },
    /// Include another config file.
    Include { path: &'a str, span: Range<usize> },
}

type ParseOutput<'a> = Vec<Instruction<'a>>;

pub fn parse<'a>(text: &'a str) -> Result<ParseOutput<'a>, Error> {
    let ctx = Context { buf: text };
    ctx.parse()
}

struct Context<'a> {
    buf: &'a str,
}

#[derive(Debug)]
pub struct Error {
    line_no: usize,
    message: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line_no + 1, self.message)
    }
}

impl std::error::Error for Error {}

impl<'a> Context<'a> {
    fn parse(&self) -> Result<ParseOutput<'a>, Error> {
        let mut output = Vec::with_capacity(self.instruction_size_hint());

        // Parser state.
        let mut section: &'a str = "";
        let mut name: &'a str = "";
        // For single or multi-line value.
        let mut value_lines: Vec<&'a str> = Vec::with_capacity(1);

        for (line_no, line) in self.buf.lines().enumerate().chain(std::iter::once((0, ""))) {
            let first_char = line.chars().next().unwrap_or('#');
            let value_empty: bool = value_lines.is_empty();
            // Multi-line config.
            if !value_empty && " \t".contains(first_char) {
                value_lines.push(line.trim());
                continue;
            }
            // Push parsed config.
            if !value_empty {
                let span = get_range(
                    self.buf,
                    value_lines.first().unwrap(),
                    value_lines.last().unwrap(),
                );
                let value = if value_lines.len() == 1 {
                    Cow::Borrowed(value_lines[0])
                } else {
                    // Strip empty lines at the end.
                    let mut n = value_lines.len();
                    while n > 0 && value_lines[n - 1].is_empty() {
                        n -= 1;
                    }
                    Cow::Owned(value_lines[..n].join("\n"))
                };
                let inst = Instruction::SetConfig {
                    section,
                    name,
                    value,
                    span,
                };
                output.push(inst);
                value_lines.clear();
            }
            // Handle different lines.
            match first_char {
                // [section] (space)
                '[' => {
                    let rest;
                    (section, rest) = match line[1..].split_once(']') {
                        None => {
                            return Err(Error {
                                line_no,
                                message: "missing ']' for section header",
                            });
                        }
                        Some((section, rest)) => (section.trim(), rest.trim()),
                    };
                    if !rest.is_empty() {
                        return Err(Error {
                            line_no,
                            message: "extra content after section header",
                        });
                    }
                    if section.is_empty() {
                        return Err(Error {
                            line_no,
                            message: "empty section name",
                        });
                    }
                }
                // # comment
                ';' | '#' => {
                    continue;
                }
                // blank line
                ' ' | '\t' => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    return Err(Error {
                        line_no,
                        message: "indented line is not part of a multi-line config",
                    });
                }
                // %include or %unset
                '%' => {
                    if let Some(rest) = line.strip_prefix("%include ") {
                        let path = rest.trim();
                        let span = get_range(self.buf, path, path);
                        let inst = Instruction::Include { path, span };
                        output.push(inst);
                    } else if let Some(rest) = line.strip_prefix("%unset ") {
                        let name = rest.trim();
                        let span = get_range(self.buf, name, name);
                        if name.contains('=') {
                            return Err(Error {
                                line_no,
                                message: "config name cannot include '='",
                            });
                        }
                        let inst = Instruction::UnsetConfig {
                            section,
                            name,
                            span,
                        };
                        output.push(inst);
                    } else {
                        return Err(Error {
                            line_no,
                            message: "unknown directive (expect '%include' or '%unset')",
                        });
                    }
                }
                // name = value
                _ => {
                    let value;
                    (name, value) = match line.split_once('=') {
                        None => {
                            return Err(Error {
                                line_no,
                                message: "expect '[section]' or 'name = value'",
                            });
                        }
                        Some((name, value)) => (name.trim(), value.trim()),
                    };
                    if name.is_empty() {
                        return Err(Error {
                            line_no,
                            message: "empty config name",
                        });
                    }
                    value_lines.push(value);
                }
            }
        }

        Ok(output)
    }

    fn instruction_size_hint(&self) -> usize {
        self.buf
            .lines()
            .filter(|l| l.starts_with('%') || l.contains('='))
            .count()
    }
}

/// Figure out a range in `text` so `text[range]` starts with the first byte
/// of `start` and ends with the last byte of `end`.
/// Assumes `start` and `end` are derived from (sub-strings of) `text`.
fn get_range(text: &str, start: &str, end: &str) -> Range<usize> {
    let text_offset: usize = text.as_ptr() as usize;
    let start_offset: usize = start.as_ptr() as usize;
    let end_offset: usize = (end.as_ptr() as usize) + end.len();
    assert!(end_offset <= text_offset + text.len());
    assert!(start_offset >= text_offset);
    assert!(start_offset <= end_offset);
    (start_offset - text_offset)..(end_offset - text_offset)
}
