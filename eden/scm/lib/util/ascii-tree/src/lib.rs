/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Draw ASCII representation of a tree.
//!
//! The tree is usually a call graph with performance information.
//!
//! Refer to tests for some examples.

mod ascii_options;
pub(crate) mod row;
mod tree;
mod tree_span;

pub use ascii_options::AsciiOptions;
pub use tree::DescribeTreeSpan;
pub use tree::Tree;
pub use tree_span::TreeSpan;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let mut tree = Tree::default();
        for (parent, start_time, duration, extra) in [
            (0, 0, 600, "_start"),
            (1, 0, 600, "main"),
            (2, 0, 100, "work1"),
            (2, 100, 200, "work2"),
            (2, 300, 300, "work1"),
        ] {
            let span = TreeSpan {
                start_time,
                duration,
                extra: Some(extra),
                ..Default::default()
            };
            tree.push(parent, span);
        }

        struct Desc;

        impl DescribeTreeSpan<&'static str> for Desc {
            fn name(&self, span: &TreeSpan<&'static str>) -> String {
                span.extra.unwrap_or_default().to_string()
            }

            fn source(&self, _span: &TreeSpan<&'static str>) -> String {
                "?".to_string()
            }
        }

        let render = |min_duration_to_hide, merge| -> String {
            let mut opts = AsciiOptions::default();
            opts.min_duration_to_hide = min_duration_to_hide;
            let mut tree = tree.clone();
            if merge {
                tree.merge_children(&opts, &|n| n.extra);
            }
            let desc = Desc;
            let out = tree.render_ascii_rows(&opts, &desc);
            format!("\n{}", out)
        };

        assert_eq!(
            render(0, false),
            r#"
Start Dur.ms | Name               Source
    0   +600 | _start             ?
    0   +600 | main               ?
    0   +100  \ work1             ?
  100   +200  \ work2             ?
  300   +300  \ work1             ?
"#
        );

        assert_eq!(
            render(200, true),
            r#"
Start Dur.ms | Name               Source
    0   +600 | _start             ?
    0   +600 | main               ?
    0   +400  \ work1 (2 times)   ?
  100   +200  \ work2             ?
"#
        );

        assert_eq!(
            render(400, true),
            r#"
Start Dur.ms | Name               Source
    0   +600 | _start             ?
    0   +600 | main               ?
    0   +400 | work1 (2 times)    ?
"#
        );
    }
}
