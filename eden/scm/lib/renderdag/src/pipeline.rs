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

pub mod graph_text_renderer;
pub mod graph_to_row_shape;
pub mod prefix_lines_to_text;
pub mod row_shape_to_prefix_lines;
pub mod types;

// re-export
pub use self::graph_text_renderer::GraphTextRenderer;
pub use self::graph_to_row_shape::GraphRowShaper;
pub use self::prefix_lines_to_text::PrefixLinesToText;
pub use self::row_shape_to_prefix_lines::ascii::AsciiPrefixLineRenderer as Ascii;
pub use self::row_shape_to_prefix_lines::ascii_large::AsciiLargePrefixLineRenderer as AsciiLarge;
pub use self::row_shape_to_prefix_lines::box_drawing::BoxDrawingPrefixLineRenderer as BoxDrawing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_text_renderer_example() {
        let mut renderer = GraphTextRenderer::<&'static str, BoxDrawing>::new().configure(|o| {
            o.min_row_height = 0;
        });
        assert_eq!(
            renderer.next_text("A", vec![], "x", "commit 1"),
            "x  commit 1\n"
        );
    }
}
