/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Render a DAG into text using a pipeline:
//!
//! 1. Node stream `(node, parents)` -> `GraphRowShape`.
//!    Computes edge shapes and column layout.
//! 2. `GraphRowShape`s -> `PrefixLine`s.
//!    Converts abstract graph rows into left-side graph prefixes.
//!    Does not know about messages or node glyphs.
//! 3. `PrefixLine`s + glyph + message -> text.
//!    Produces the final lines by filling the glyph, repeating prefixes,
//!    and placing message text.
//!
//! Design principle: minimal coupling -> reusable, cacheable. For example:
//! - Step 1 is not coupled with glyph or message.
//!   It can be serialized with serde for SVG rendering.
//!   It takes options that might affect graph layout (reserved column,
//!   stagger consecutive disconnected nodes).
//! - Step 2 is not coupled with message.
//!   It can be used or cached standalone. For example, the callsite
//!   might want to show test signals per commit. It might use a placeholder
//!   initially, and replace it with the real test status later. It can cache
//!   Step 2 result (since the commit graph is not changing), and only update
//!   the message part later.

#![allow(dead_code)]

pub mod graph_to_row_shape;
pub mod types;
