//! A library for parsing programs written in the shell programming language.
//!
//! The `Parser` implementation will pass all of its intermediate parse results
//! to a `Builder` implementation, allowing the `Builder` to transform the
//! results to a desired format. This allows for customizing what AST is
//! produced without having to walk and transform an entire AST produced by
//! the parser.
//!
//! See the `Parser` documentation for more information on getting started.
//!
//! # Supported Grammar
//!
//! * Conditional lists (`foo && bar || baz`)
//! * Pipelines (`! foo | bar`)
//! * Compound commands
//!  * Brace blocks (`{ foo; }`)
//!  * Subshells (`$(foo)`)
//!  * `for` / `case` / `if` / `while` / `until`
//! * Function declarations
//! * Redirections
//! * Heredocs
//! * Comments
//! * Parameters (`$foo`, `$@`, etc.)
//! * Parameter substitutions (`${foo:-bar}`)
//! * Quoting (single, double, backticks, escaping)
//! * Arithmetic substitutions
//!  * Common arithmetic operations required by the POSIX standard
//!  * Variable expansion
//!  * **Not yet implemented**: Other inner abitrary parameter/substitution expansion

#![doc(html_root_url = "https://docs.rs/conch-parser/0.1")]
#![cfg_attr(not(test), deny(clippy::print_stdout))]
#![deny(clippy::wrong_self_convention)]
#![deny(missing_copy_implementations)]
#![deny(missing_debug_implementations)]
#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![forbid(unsafe_code)]

pub mod ast;
pub mod lexer;
pub mod parse;
pub mod token;
