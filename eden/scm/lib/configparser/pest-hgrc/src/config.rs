/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::ops::Range;

use pest::Parser;
use pest::Span;

use crate::parser::ConfigParser;
use crate::parser::Rule;

type Pair<'a> = pest::iterators::Pair<'a, Rule>;

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

pub fn parse<'a>(text: &'a str) -> Result<ParseOutput<'a>, pest::error::Error<Rule>> {
    let ctx = Context { buf: text };
    ctx.parse()
}

struct Context<'a> {
    buf: &'a str,
}

impl<'a> Context<'a> {
    fn handle_value(
        &self,
        pair: Pair<'a>,
        section: &'a str,
        name: &'a str,
        span: Range<usize>,
        output: &mut ParseOutput<'a>,
    ) {
        let pairs = pair.into_inner();
        let mut lines = Vec::with_capacity(1);
        for pair in pairs {
            if Rule::line == pair.as_rule() {
                lines.push(extract(self.buf, pair.as_span()));
            }
        }

        let value = match lines.len() {
            1 => Cow::Borrowed(strip_whitespace(lines[0], 0, lines[0].len())),
            _ => {
                // Strip empty lines at the end.
                let mut n = lines.len();
                while n > 0 && lines[n - 1].is_empty() {
                    n -= 1;
                }
                Cow::Owned(lines[..n].join("\n"))
            }
        };

        output.push(Instruction::SetConfig {
            section,
            name,
            value,
            span,
        });
    }

    fn handle_config_item(&self, pair: Pair<'a>, section: &'a str, output: &mut ParseOutput<'a>) {
        let pairs = pair.into_inner();
        let mut name = "";
        for pair in pairs {
            match pair.as_rule() {
                Rule::config_name => name = extract(self.buf, pair.as_span()),
                Rule::value => {
                    let span = pair.as_span();
                    return self.handle_value(
                        pair,
                        section,
                        name,
                        span.start()..span.end(),
                        output,
                    );
                }
                _ => (),
            }
        }
        unreachable!();
    }

    fn handle_section(&self, pair: Pair<'a>, section: &mut &'a str) {
        let pairs = pair.into_inner();
        for pair in pairs {
            if let Rule::section_name = pair.as_rule() {
                *section = extract(self.buf, pair.as_span());
                return;
            }
        }
        unreachable!();
    }

    fn handle_include(&self, pair: Pair<'a>, output: &mut ParseOutput<'a>) {
        let pairs = pair.into_inner();
        for pair in pairs {
            if let Rule::line = pair.as_rule() {
                let span = to_std_span(pair.as_span());
                let path = pair.as_str();
                output.push(Instruction::Include { path, span });
            }
        }
    }

    fn handle_unset(&self, pair: Pair<'a>, section: &'a str, output: &mut ParseOutput<'a>) {
        let unset_span = pair.as_span();
        let pairs = pair.into_inner();
        for pair in pairs {
            if let Rule::config_name = pair.as_rule() {
                let name = extract(self.buf, pair.as_span());
                let span = unset_span.start()..unset_span.end();
                output.push(Instruction::UnsetConfig {
                    section,
                    name,
                    span,
                });
                return;
            }
        }
        unreachable!();
    }

    fn handle_directive(&self, pair: Pair<'a>, section: &'a str, output: &mut ParseOutput<'a>) {
        let pairs = pair.into_inner();
        for pair in pairs {
            match pair.as_rule() {
                Rule::include => self.handle_include(pair, output),
                Rule::unset => self.handle_unset(pair, section, output),
                _ => {}
            }
        }
    }

    fn handle_file(&self, pair: Pair<'a>, section: &mut &'a str, output: &mut ParseOutput<'a>) {
        match pair.as_rule() {
            Rule::config_item => self.handle_config_item(pair, section, output),
            Rule::section => self.handle_section(pair, section),
            Rule::directive => self.handle_directive(pair, section, output),
            Rule::blank_line | Rule::comment_line | Rule::new_line | Rule::EOI => {}

            Rule::comment_start
            | Rule::compound
            | Rule::config_name
            | Rule::equal_sign
            | Rule::file
            | Rule::include
            | Rule::left_bracket
            | Rule::line
            | Rule::right_bracket
            | Rule::section_name
            | Rule::space
            | Rule::unset
            | Rule::value => unreachable!(),
        }
    }

    fn parse(&self) -> Result<ParseOutput<'a>, pest::error::Error<Rule>> {
        let mut output = Vec::new();
        let mut section = "";
        let pairs = ConfigParser::parse(Rule::file, self.buf)?;
        for pair in pairs {
            self.handle_file(pair, &mut section, &mut output);
        }
        Ok(output)
    }
}

/// Remove space characters from both ends. Remove newline characters from the end.
/// `start` position is inclusive, `end` is exclusive.
fn strip_whitespace(buf: &str, start: usize, end: usize) -> &str {
    let slice: &str = &buf[start..end];
    slice
        .trim_start_matches(|c| c == '\t' || c == ' ')
        .trim_end_matches(|c| " \t\r\n".contains(c))
}

/// Extract text from a larger buffer, with spaces stripped.
fn extract<'a>(buf: &'a str, span: Span<'a>) -> &'a str {
    strip_whitespace(buf, span.start(), span.end())
}

fn to_std_span(span: pest::Span) -> Range<usize> {
    span.start()..span.end()
}
