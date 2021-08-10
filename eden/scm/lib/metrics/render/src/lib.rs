/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metrics::{Counter, Registry};
use progress_model::Registry as ProgressRegistry;
use progress_model::{IoSample, IoTimeSeries};
use std::collections::HashSet;
use std::env;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use tracing::error;

/// Initializes metrics reporting while guard is not dropped.
/// Configuration is read from env variable EDENSCM_METRICS.
/// This variable holds coma separated list of reporting parameters in form Filter:Reporter.
///
/// Filter is the name of the metric, either full name, or a prefix (crate name without '.').
/// Reporter is optional, by default progress reporter is used - it renders progress bar with the counter.
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
}

impl Renderer {
    pub fn render(&mut self, metrics: Vec<(&'static str, &'static Counter)>) {
        match self {
            Self::ProgressBar(inner) => Self::render_progress(inner, metrics),
        }
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
            let time_series = IoTimeSeries::new(visible_name, "");
            let task =
                time_series.async_sampling(take_sample, IoTimeSeries::default_sample_interval());
            async_runtime::spawn(task);

            progress_registry.register_io_time_series(&time_series);
        }
    }
}
