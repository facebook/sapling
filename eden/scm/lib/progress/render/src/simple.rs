/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Simple renderer. Does not use complex ANSI escape codes (ex. colors).

use std::borrow::Cow;
use std::sync::Arc;

use progress_model::CacheStats;
use progress_model::IoTimeSeries;
use progress_model::ProgressBar;
use progress_model::Registry;
use progress_model::TimeSeriesMode;
use termwiz::surface::Change;

use crate::RenderingConfig;

/// Render progress in to a list of termwiz changes.
pub fn render(registry: &Registry, config: &RenderingConfig) -> Vec<Change> {
    vec![render_string(registry, config).into()]
}

/// Render progress into a multi-line string.
pub fn render_string(registry: &Registry, config: &RenderingConfig) -> String {
    let mut lines = Vec::new();

    let cache_list = registry.list_cache_stats();
    let series_list = registry.list_io_time_series();
    let bar_list = registry.list_progress_bar();

    render_cache_stats(&mut lines, &cache_list, config);
    render_time_series(&mut lines, &series_list, config);
    render_progress_bars(&mut lines, &bar_list, config);

    for line in lines.iter_mut() {
        *line = config.truncate_line(&line).to_string();
    }

    lines.join("\n")
}

fn render_time_series(
    lines: &mut Vec<String>,
    series_list: &[Arc<IoTimeSeries>],
    config: &RenderingConfig,
) {
    for model in series_list {
        let mut phrases = Vec::with_capacity(4);
        if model.is_stale() {
            continue;
        }

        // Net [▁▂▄█▇▅▃▆] 3 MB/s
        phrases.push(format!("{:>1$}", model.topic(), config.max_topic_len()));

        let ascii = ascii_time_series(&model);
        phrases.push(format!("[{}]", ascii));

        let speed = match model.mode() {
            TimeSeriesMode::BytesSpeed => {
                let (rx, tx) = model.bytes_per_second();
                human_rx_tx_per_second(rx, tx)
            }
            TimeSeriesMode::ValueNoUnit => format!("{}", model.total_bytes()),
        };
        if !speed.is_empty() {
            phrases.push(speed);
        }

        let count = model.count();
        if count > 1 {
            let unit = model.count_unit();
            phrases.push(format!("{} {}", count, unit));
        }

        match model.mode() {
            TimeSeriesMode::BytesSpeed => {
                let total = human_rx_tx_total(model.input_bytes(), model.output_bytes());
                if !total.is_empty() {
                    phrases.push(format!("total {}", total));
                }
            }
            _ => {}
        }

        let line = phrases.join("  ");
        lines.push(line);
    }
}

fn render_progress_bars(
    lines: &mut Vec<String>,
    bars: &[Arc<ProgressBar>],
    config: &RenderingConfig,
) {
    let mut hidden = 0;
    let mut shown = 0;
    for bar in bars.iter() {
        if config.delay.as_millis() > 0 && bar.elapsed() < config.delay {
            continue;
        }

        if shown >= config.max_bar_count {
            hidden += 1;
            continue;
        }

        shown += 1;

        // topic [====>    ] 12 / 56 files message
        let mut topic = bar.topic();
        while topic.len() > config.max_topic_len() {
            match topic.rfind(char::is_whitespace) {
                Some(idx) => topic = &topic[..idx],
                None => break,
            }
        }
        let mut phrases = vec![format!("{:>1$}", capitalize(topic), config.max_topic_len())];
        // [===>    ]

        let (pos, total) = bar.position_total();
        let width = 15usize;
        if total > 0 && pos <= total {
            let (len, end) = if pos == total {
                (width, "")
            } else {
                ((pos * (width as u64) / total) as usize, ">")
            };
            phrases.push(format!(
                "[{}{}{}]",
                str::repeat("=", len),
                end,
                str::repeat(" ", width - len - end.len())
            ));
        } else {
            // Spinner
            let pos = if cfg!(test) {
                5
            } else {
                bar.elapsed().as_millis() / 200
            };
            let spaceship = "<=>";
            let left_max = width - spaceship.len();
            // 0, 1, 2, ..., width - 4, width - 3, width - 4, ..., 0
            let mut left_pad = (pos as usize) % (left_max * 2);
            if left_pad >= left_max {
                left_pad = 2 * left_max - left_pad;
            }
            phrases.push(format!(
                "[{}{}{}]",
                str::repeat(" ", left_pad),
                spaceship,
                str::repeat(" ", left_max - left_pad)
            ));
        }

        // 12 / 56 files
        phrases.push(crate::unit::unit_phrase(bar.unit(), pos, total));

        // message
        if let Some(message) = bar.message() {
            phrases.push(message.to_string());
        }
        lines.push(phrases.join("  "));
    }

    if hidden > 0 {
        lines.push(format!(
            "{:>width$}  and {} more",
            "",
            hidden,
            width = config.max_topic_len()
        ));
    }
}

fn render_cache_stats(lines: &mut Vec<String>, list: &[Arc<CacheStats>], config: &RenderingConfig) {
    for model in list {
        // topic [====>    ] 12 / 56 files message
        let topic = model.topic();
        let miss = model.miss();
        let hit = model.hit();
        let total = miss + hit;
        if total > 0 {
            let mut line = format!(
                "{:>width$}  {}",
                topic,
                total,
                width = config.max_topic_len()
            );
            if miss > 0 {
                let miss_rate = (miss * 100) / (total.max(1));
                line += &format!(" ({}% miss)", miss_rate);
            }
            lines.push(line);
        }
    }
}

fn human_rx_tx_per_second(rx: u64, tx: u64) -> String {
    let mut result = Vec::new();
    for (speed, symbol) in [(rx, '▼'), (tx, '▲')] {
        if speed > 0 {
            result.push(format!("{} {}", symbol, human_bytes_per_second(speed)));
        }
    }
    result.join("  ")
}

fn human_rx_tx_total(rx: u64, tx: u64) -> String {
    let mut result = Vec::new();
    for (total, dir) in [(rx, "down"), (tx, "up")] {
        if total > 0 {
            result.push(format!("{} {}", crate::unit::human_bytes(total), dir));
        }
    }
    result.join(", ")
}

fn human_bytes_per_second(bytes_per_second: u64) -> String {
    format!("{}/s", crate::unit::human_bytes(bytes_per_second))
}

fn ascii_time_series(time_series: &IoTimeSeries) -> String {
    const GAUGE_CHARS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let v = time_series.scaled_speeds((GAUGE_CHARS.len() - 1) as u8);
    v.into_iter().map(|i| GAUGE_CHARS[i as usize]).collect()
}

fn capitalize<'a>(s: &'a str) -> Cow<'a, str> {
    if s.chars().next().unwrap_or('A').is_ascii_uppercase() {
        Cow::Borrowed(s)
    } else {
        let mut first = true;
        let s: String = s
            .chars()
            .map(|c| {
                if first {
                    first = false;
                    c.to_ascii_uppercase()
                } else {
                    c
                }
            })
            .collect();
        Cow::Owned(s)
    }
}
