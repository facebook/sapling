/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Compute pop_out value for our children bars.
// - `pop_out` is our pop_out value (from our parents).
// - `is_final_bar` is whether we are the last bar at our depth.
fn compute_pop_out(pop_out: usize, is_final_bar: bool) -> usize {
    if !is_final_bar {
        1
    } else if pop_out > 0 {
        pop_out + 1
    } else {
        0
    }
}

// Preamble that should be drawn before progress bar to make nested nature clear/pretty.
// - `depth` is nesting depth where 0 is root level.
// - `is_last` is whether this bar is the last bar at this depth, and has no children.
// - `is_first` is whether this bar is the first bar at this depth.
// - `pop_out` is the depth reduction for the next bar after this `depth`, if any.
fn bar_prefix(depth: usize, is_last: bool, is_first: bool, pop_out: usize) -> String {
    let mut prefix = String::new();

    if is_last && pop_out > 0 {
        // Draw indenting spaces leaving room for "pop-out" line.
        prefix.push_str(&"  ".repeat(depth.saturating_sub(pop_out)));

        if !is_first || pop_out > 1 {
            // Draw "pop-out" line that precedes our bar, pointing to the bar
            // on the next line.
            prefix.push_str("╭─");
            prefix.push_str(&"──".repeat(pop_out.saturating_sub(2)));
        }
    } else if is_first && depth > 0 {
        // If we are first bar at nested depth, leave room for the "pop-in" line.
        prefix.push_str(&"  ".repeat(depth.saturating_sub(1)));
    } else {
        // No popping - draw full indent.
        prefix.push_str(&"  ".repeat(depth));
    }

    if is_first && depth > 0 {
        // Draw the "pop-in" line from previous bar to us.

        if !is_last || pop_out == 0 {
            prefix.push_str("╰─");
        } else if pop_out > 1 {
            prefix.push_str("┴─");
        } else {
            prefix.push_str("├─");
        }
    }

    // Draw the final symbol directly preceeding our bar.
    if is_first && is_last {
        prefix.push_str("─ ");
    } else if is_first && depth == 0 {
        prefix.push_str("╭ ");
    } else if is_first && depth > 0 {
        prefix.push_str("┬ ");
    } else if !is_last {
        prefix.push_str("├ ");
    } else if pop_out > 0 {
        prefix.push_str("┴ ")
    } else {
        prefix.push_str("╰ ");
    }

    prefix
}

// Return postamble that should follow the progress bar to make things pretty.
// See `bar_prefix` for explanation of arguments.
fn bar_suffix(depth: usize, is_last: bool, is_first: bool, pop_out: usize) -> &'static str {
    if depth == 0 {
        if is_first && is_last {
            " ─"
        } else if is_first {
            " ╮"
        } else if is_last {
            " ╯"
        } else {
            " ┤"
        }
    } else if is_last && pop_out == 0 {
        " ╯"
    } else {
        " ┤"
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct StubBar(String, Vec<StubBar>);

    macro_rules! bar {
        ($topic:tt $(, $child:expr )*) => {
            StubBar($topic.to_string(), vec![$($child,)*])
        };
    }

    fn render(bars: &[StubBar]) -> String {
        fn inner(bars: &[StubBar], depth: usize, pop_out: usize) -> String {
            let mut output = Vec::new();
            for (idx, b) in bars.iter().enumerate() {
                let is_last = idx == bars.len() - 1 && b.1.is_empty();
                let is_first = idx == 0;

                let mut rendered = bar_prefix(depth, is_last, is_first, pop_out);

                let width = 10 - 2 * depth;

                rendered.push_str(&format!("{:<width$}", b.0));
                rendered.push_str(bar_suffix(depth, is_last, is_first, pop_out));

                output.push(rendered);

                if !b.1.is_empty() {
                    output.push(inner(
                        &b.1,
                        depth + 1,
                        compute_pop_out(pop_out, idx == bars.len() - 1),
                    ))
                }
            }

            output.join("\n")
        }

        let output = inner(bars, 0, 0);

        if !output.is_empty() {
            format!("\n{}\n", output)
        } else {
            output
        }
    }

    #[test]
    fn test_nesting() {
        assert_eq!(render(&[]), r"");

        assert_eq!(
            render(&[bar!("A")]),
            r"
─ A          ─
",
        );

        assert_eq!(
            render(&[bar!("A"), bar!("B")]),
            r"
╭ A          ╮
╰ B          ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1"))]),
            r"
╭ A          ╮
╰── A.1      ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1")), bar!("B")]),
            r"
╭ A          ╮
├── A.1      ┤
╰ B          ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1"), bar!("A.2"))]),
            r"
╭ A          ╮
╰─┬ A.1      ┤
  ╰ A.2      ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1"), bar!("A.2"), bar!("A.3"))]),
            r"
╭ A          ╮
╰─┬ A.1      ┤
  ├ A.2      ┤
  ╰ A.3      ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1"), bar!("A.2")), bar!("B")]),
            r"
╭ A          ╮
╰─┬ A.1      ┤
╭─┴ A.2      ┤
╰ B          ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1"), bar!("A.2"), bar!("A.3")), bar!("B")]),
            r"
╭ A          ╮
╰─┬ A.1      ┤
  ├ A.2      ┤
╭─┴ A.3      ┤
╰ B          ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("A.1", bar!("A.1.1"))), bar!("B")]),
            r"
╭ A          ╮
╰─┬ A.1      ┤
╭─┴── A.1.1  ┤
╰ B          ╯
"
        );

        assert_eq!(
            render(&[bar!("A", bar!("1", bar!("2", bar!("3")))), bar!("B")]),
            r"
╭ A          ╮
╰─┬ 1        ┤
  ╰─┬ 2      ┤
╭───┴── 3    ┤
╰ B          ╯
"
        );
    }
}
