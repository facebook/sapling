/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;

use progress_model::BarState;
use progress_model::ProgressBar;
use progress_model::Registry;
use termwiz::cell::AttributeChange;
use termwiz::cell::Intensity;
use termwiz::color::AnsiColor;
use termwiz::color::ColorAttribute;
use termwiz::surface::Change;
use termwiz::surface::change::ChangeSequence;
use unicode_segmentation::UnicodeSegmentation;

use crate::RenderingConfig;
use crate::maybe_pad;
use crate::unit::human_duration;

const SPINNER: &[&str] = &["⠉", "⠙", "⠹", "⠸", "⠼", "⠴", "⠤", "⠦", "⠧", "⠇", "⠏", "⠋"];
const SPINNER_INTERVAL_MS: u128 = 100;

const MAX_TOPIC_LENGTH: usize = 50;
const MIN_TOPIC_LENGTH: usize = 30;

pub fn render(registry: &Registry, config: &RenderingConfig) -> Vec<Change> {
    let mut changes = ChangeSequence::new(config.term_height, config.term_width);

    // Defer to simple rendering for non-progress bars for now.
    // TODO: make rendering style match that of structured bars.
    let mut non_progress = Vec::new();
    crate::simple::render_cache_stats(&mut non_progress, &registry.list_cache_stats(), config);
    crate::simple::render_time_series(&mut non_progress, &registry.list_io_time_series(), 0);
    if !non_progress.is_empty() {
        // Add empty line separating non-progress bars from progress bars.
        // Maybe get rid of this when we unify styling.
        non_progress.push(String::new());
    }
    for line in non_progress {
        changes.add(format!("{line}\r\n"));
    }

    render_progress_bars(&mut changes, &registry.list_progress_bar(), config);

    changes.consume()
}

fn render_progress_bars(
    changes: &mut ChangeSequence,
    bars: &[Arc<ProgressBar>],
    config: &RenderingConfig,
) {
    let mut children = HashMap::<u64, Vec<Arc<ProgressBar>>>::new();
    let mut root_bars = Vec::new();
    let mut topic_length = MIN_TOPIC_LENGTH;
    for bar in bars {
        // Filter bars early that we aren't going to render.
        if bar.adhoc()
            && config.delay.as_millis() > 0
            && bar.since_start().unwrap_or_default() < config.delay
        {
            continue;
        }

        topic_length = topic_length.max(bar.topic().graphemes(true).count());

        if let Some(parent) = bar.parent() {
            children.entry(parent.id()).or_default().push(bar.clone());
        } else {
            root_bars.push(bar.clone());
        }
    }

    topic_length = topic_length.min(MAX_TOPIC_LENGTH);

    // Max budget for the elapsed time (which appears next to topic).
    const MAX_ELAPSED_TIME_LENGTH: usize = 6;

    // Add in space for elapsed time as necessary so the elapsed time's length doesn't
    // count against MAX_TOPIC_LENGTH.
    topic_length += (topic_length + MAX_ELAPSED_TIME_LENGTH).saturating_sub(MAX_TOPIC_LENGTH);

    let mut renderer = Renderer {
        changes,
        config,
        topic_length,
        rendered_so_far: 0,
    };
    renderer.render_bars(&root_bars, &children, 0, 0);
}

struct Renderer<'a> {
    changes: &'a mut ChangeSequence,
    config: &'a RenderingConfig,
    topic_length: usize,
    rendered_so_far: usize,
}

impl Renderer<'_> {
    fn render_bars(
        &mut self,
        bars: &[Arc<ProgressBar>],
        id_to_children: &HashMap<u64, Vec<Arc<ProgressBar>>>,
        depth: usize,
        pop_out: usize,
    ) {
        // Are we the first bar being rendered (at this depth).
        let mut is_first = true;

        for (idx, bar) in bars.iter().enumerate() {
            if self.rendered_so_far >= self.config.max_bar_count {
                return;
            }

            self.rendered_so_far += 1;

            match bar.state() {
                BarState::Pending => self.changes.add(" "),
                BarState::Running => {
                    let spin_idx = (bar.since_creation().as_millis() / SPINNER_INTERVAL_MS)
                        as usize
                        % SPINNER.len();
                    self.changes
                        .add(AttributeChange::Intensity(Intensity::Half));
                    self.changes.add(SPINNER[spin_idx]);
                }
                BarState::Complete => {
                    self.changes
                        .add(AttributeChange::Foreground(AnsiColor::Green.into()));
                    self.changes.add("✓");
                }
            }

            self.changes.add(Change::AllAttributes(Default::default()));
            self.changes.add(" ");

            // See `bar_prefix` for a description of these values.
            let (pop_out, is_last) = if self.rendered_so_far >= self.config.max_bar_count {
                (0, true)
            } else {
                (
                    pop_out,
                    idx == bars.len() - 1 && id_to_children.get(&bar.id()).is_none(),
                )
            };

            self.changes
                .add(bar_prefix(depth, is_last, is_first, pop_out));

            // Format the elapsed time. Include space prefix to guarantee separation from
            // the topic text.
            let since_start = format!(
                "  {}",
                bar.since_start().map(human_duration).unwrap_or_default()
            );

            // Trim off topic length based on depth. The left side of bar is indented, but
            // we keep the right side of bars aligned (causing nested bars to get
            // shorter).
            let topic_length = self.topic_length - 2 * depth;

            let (pos, total) = bar.position_total();
            if total > 0 {
                // Here we draw the actual advancing progress bar. We use a
                // green background to represent the progress.

                self.changes
                    .add(AttributeChange::Background(AnsiColor::Green.into()));
                self.changes
                    .add(AttributeChange::Foreground(AnsiColor::Black.into()));

                let bg_len = ((pos as f64 / total as f64) * topic_length as f64) as usize;
                let mut graphemes = bar.topic().graphemes(true);

                for i in 0..topic_length {
                    if i == bg_len {
                        self.changes
                            .add(AttributeChange::Background(ColorAttribute::Default));
                        self.changes
                            .add(AttributeChange::Foreground(ColorAttribute::Default));
                    }

                    if topic_length - i == since_start.len() {
                        // Here we mix in the elapsed time so it shows up
                        // right-justified within the progress bar. This will
                        // cut off a long topic.
                        graphemes = since_start.graphemes(true);
                        self.changes
                            .add(AttributeChange::Intensity(Intensity::Half));
                    }

                    self.changes.add(graphemes.next().unwrap_or(" "));
                }
            } else {
                // We are a spinner with no real progress.
                self.changes.add(format!(
                    "{:length$.length$}",
                    bar.topic(),
                    length = topic_length - since_start.len()
                ));
                self.changes
                    .add(AttributeChange::Intensity(Intensity::Half));
                self.changes.add(since_start);
            }

            self.changes.add(Change::AllAttributes(Default::default()));

            let line = format!(
                "{}{}{}",
                bar_suffix(depth, is_last, is_first, pop_out),
                maybe_pad(crate::unit::unit_phrase(bar.unit(), pos, total)),
                maybe_pad(bar.message().unwrap_or_default().as_ref()),
            );

            let (current_x, _) = self.changes.current_cursor_position();
            self.changes.add(self.config.truncate_by_width(
                &line,
                self.config.term_width - current_x,
                "…",
            ));

            self.changes.add("\r\n");

            if let Some(children) = id_to_children.get(&bar.id()) {
                self.render_bars(
                    children,
                    id_to_children,
                    depth + 1,
                    compute_pop_out(pop_out, idx == bars.len() - 1),
                );
            }

            is_first = false;
        }
    }
}

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

    // Draw the final symbol directly preceding our bar.
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
    use std::io::Write;
    use std::io::{self};
    use std::time::Duration;

    use progress_model::ProgressBarBuilder;
    use termwiz::caps::Capabilities;
    use termwiz::render::RenderTty;
    use termwiz::render::terminfo::TerminfoRenderer;

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

    struct DumbTty<'a> {
        w: &'a mut dyn Write,
    }

    impl RenderTty for DumbTty<'_> {
        fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
            Ok((80, 26))
        }
    }

    impl io::Write for DumbTty<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.w.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.w.flush()
        }
    }

    #[test]
    fn test_filtering_children() {
        let registry = Registry::default();

        // Parent is visible, child is not visible yet (due to delay).
        let _parent = ProgressBarBuilder::new()
            .topic("parent")
            .registry(&registry)
            .adhoc(false)
            .active();
        let _child = ProgressBarBuilder::new()
            .topic("child")
            .registry(&registry)
            .adhoc(true)
            .active();

        let mut changes = ChangeSequence::new(100, 100);
        render_progress_bars(
            &mut changes,
            &registry.list_progress_bar(),
            &RenderingConfig {
                delay: Duration::from_secs(5),
                ..Default::default()
            },
        );

        let mut renderer =
            TerminfoRenderer::new(Capabilities::new_with_hints(Default::default()).unwrap());
        let mut buf = Vec::new();
        renderer
            .render_to(&changes.consume(), &mut DumbTty { w: &mut buf })
            .unwrap();

        let got = std::str::from_utf8(buf.as_ref()).unwrap();
        // Be sure we draw with "-", not "╭" (i.e. hidden child should not influence rendering).
        assert!(got.contains("─ parent"), "{got}");
    }

    #[test]
    fn test_long_lines() {
        let registry = Registry::default();

        let bar = ProgressBarBuilder::new()
            .topic("topic")
            .registry(&registry)
            .adhoc(false)
            .active();

        bar.set_message("really long".repeat(100));

        let mut changes = ChangeSequence::new(26, 80);
        render_progress_bars(
            &mut changes,
            &registry.list_progress_bar(),
            &RenderingConfig::default(),
        );

        let mut renderer =
            TerminfoRenderer::new(Capabilities::new_with_hints(Default::default()).unwrap());
        let mut buf = Vec::new();
        renderer
            .render_to(&changes.consume(), &mut DumbTty { w: &mut buf })
            .unwrap();

        // Make sure we truncate long lines.
        let got = std::str::from_utf8(buf.as_ref()).unwrap();
        assert!(
            got.trim_end()
                .ends_with("─ really longreally longreally longreally l…"),
            "{got}"
        );
    }

    #[test]
    fn test_long_topic() {
        // test both spinner and non-spinner case
        for spinner in [true, false] {
            let registry = Registry::default();

            let _parent = ProgressBarBuilder::new()
                .topic("A".repeat(MAX_TOPIC_LENGTH))
                .registry(&registry)
                .adhoc(false)
                .total(if spinner { 0 } else { 1 })
                .active();

            let _child = ProgressBarBuilder::new()
                .topic("B".repeat(MAX_TOPIC_LENGTH))
                .registry(&registry)
                .adhoc(false)
                .total(if spinner { 0 } else { 1 })
                .active();

            let mut changes = ChangeSequence::new(26, 80);
            render_progress_bars(
                &mut changes,
                &registry.list_progress_bar(),
                &RenderingConfig::default(),
            );

            let mut renderer =
                TerminfoRenderer::new(Capabilities::new_with_hints(Default::default()).unwrap());
            let mut buf = Vec::new();
            renderer
                .render_to(&changes.consume(), &mut DumbTty { w: &mut buf })
                .unwrap();

            let got = String::from_utf8(buf)
                .unwrap()
                .chars()
                .filter(|c| !c.is_ascii_control())
                .collect::<String>();

            // Has two spaces separating topic and elapsed time.
            assert!(
                got.contains("╭ AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA[0m[2m  0.0s"),
                "{got}"
            );
            assert!(
                got.contains("╰── BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB[0m[2m  0.0s"),
                "{got}"
            );
        }
    }
}
