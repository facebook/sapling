/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::env;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use metrics::Counter;
use metrics::Registry;
use once_cell::sync::Lazy;
use progress_model::IoSample;
use progress_model::IoTimeSeries;
use progress_model::Registry as ProgressRegistry;
use progress_model::TimeSeriesMode;
use tracing::error;

/// Initializes metrics reporting while guard is not dropped.
/// Configuration is read from env variable EDENSCM_METRICS.
/// This variable holds coma separated list of reporting parameters in form Filter:Reporter.
///
/// Filter is the name of the metric, either full name, or a prefix (crate name without '.').
/// Reporter is optional, by default progress reporter is used - it renders progress bar with the counter.
///
/// Available renderers:
/// p | progress = render interactively using progress crate.
/// l | log = periodically print available counters. This best works for non-interactive executions like when debugging performance inside EdenFS
pub fn init_from_env(guard: Weak<()>) {
    if let Ok(config) = env::var("EDENSCM_METRICS") {
        init(&config, guard)
    }
}

fn init(config: &str, guard: Weak<()>) {
    let config = parse_config(config);
    if config.is_empty() {
        return;
    }
    thread::Builder::new()
        .name("metrics-report".to_string())
        .spawn(move || worker(config, guard))
        .unwrap();
}

fn worker(mut config: Vec<(MetricsFilter, Renderer)>, guard: Weak<()>) {
    let registry = Registry::global();
    while guard.upgrade().is_some() {
        let metrics = registry.counters();
        for (filter, renderer) in &mut config {
            let filtered = metrics.iter().filter_map(|(name, v)| {
                if filter.matches(name) {
                    Some((*name, *v))
                } else {
                    None
                }
            });
            renderer.render(filtered.collect());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn parse_config(config: &str) -> Vec<(MetricsFilter, Renderer)> {
    let mut result = vec![];
    for item in config.split(',') {
        let mut split = item.split(':');
        let filter = split.next().expect("First element in split always exist");
        let render_type = split.next();
        let renderer = match render_type {
            None | Some("p") | Some("progress") => {
                if split.next().is_some() {
                    error!("Progress render does not have parameters, extra parameters ignored");
                }
                Renderer::ProgressBar(Default::default())
            }
            Some("l") | Some("log") => Renderer::Log,
            Some(other) => {
                error!("Invalid metrics renderer: {}", other);
                continue;
            }
        };
        let filter = MetricsFilter::parse(filter);
        result.push((filter, renderer));
    }
    result
}

struct MetricsFilter {
    prefix: String,
    exact: bool, // Either exact(true) or prefix(false)
}

impl MetricsFilter {
    pub fn parse(s: &str) -> MetricsFilter {
        let exact = s.contains('.');
        MetricsFilter {
            prefix: s.to_string(),
            exact,
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        if self.exact {
            self.prefix.as_str() == name
        } else {
            name.starts_with(&self.prefix)
        }
    }
}

enum Renderer {
    ProgressBar(HashSet<&'static str>),
    Log,
}

impl Renderer {
    pub fn render(&mut self, metrics: Vec<(&'static str, &'static Counter)>) {
        match self {
            Self::ProgressBar(inner) => Self::render_progress(inner, metrics),
            Self::Log => Self::render_log(metrics),
        }
    }

    fn render_log(metrics: Vec<(&'static str, &'static Counter)>) {
        use std::fmt::Write;

        const LINE_LENGTH: usize = 120;
        static STARTED: Lazy<Instant> = Lazy::new(Instant::now);

        let started = *STARTED;
        if metrics.is_empty() {
            return;
        }

        let timestamp_ms = started.elapsed().as_millis();
        let mut lines = vec![];
        let mut current_line = String::with_capacity(LINE_LENGTH);
        write!(current_line, "T{:0>8}{: <31}", timestamp_ms, "").ok(); // 40 symbols
        for (name, counter) in metrics {
            if current_line.len() >= LINE_LENGTH {
                lines.push(current_line);
                current_line = String::with_capacity(LINE_LENGTH);
            }
            write!(current_line, "{:.<32}{: <8}", name, counter.value()).ok(); // 40 symbols
        }
        lines.push(current_line);
        println!("{}", lines.join("\n"));
    }

    fn render_progress(
        inner: &mut HashSet<&'static str>,
        metrics: Vec<(&'static str, &'static Counter)>,
    ) {
        let progress_registry = ProgressRegistry::main();
        for (name, counter) in metrics {
            if !inner.insert(name) {
                continue;
            }
            let take_sample = move || {
                // This abuses API little bit by supplying random counter into IoSample, need to fix later
                IoSample::from_io_bytes(counter.value() as _, 0)
            };
            let visible_name = format!("{:.<32}", name);
            let time_series =
                IoTimeSeries::new_with_mode(visible_name, "", TimeSeriesMode::ValueNoUnit);
            let task =
                time_series.async_sampling(take_sample, IoTimeSeries::default_sample_interval());
            async_runtime::spawn(task);

            progress_registry.register_io_time_series(&time_series);
        }
    }
}
