/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::OnceLock;

use sampling::SamplingConfig;
use tracing::subscriber::Interest;
use tracing::Event;
use tracing::Metadata;
use tracing::Subscriber;
use tracing_serde::fields::AsMap;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Filter;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

pub struct SamplingLayer {
    config: &'static OnceLock<Option<Arc<SamplingConfig>>>,
}

impl SamplingLayer {
    pub fn new<S: Subscriber + for<'a> LookupSpan<'a>>() -> impl Layer<S> {
        Self::new_with_config(&sampling::CONFIG)
    }

    fn new_with_config<S: Subscriber + for<'a> LookupSpan<'a>>(
        config: &'static OnceLock<Option<Arc<SamplingConfig>>>,
    ) -> impl Layer<S> {
        Self { config }.with_filter(SamplingFilter { config })
    }
}

impl<S: Subscriber> Layer<S> for SamplingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let config = match self.config.get() {
            Some(Some(sc)) => sc.clone(),
            _ => return,
        };

        let category = match config.category(event.metadata().target()) {
            Some(v) => v,
            None => return,
        };

        let _ = config.append(category, &event.field_map());
    }
}

struct SamplingFilter {
    config: &'static OnceLock<Option<Arc<SamplingConfig>>>,
}

impl SamplingFilter {
    fn is_enabled(&self, meta: &Metadata<'_>) -> bool {
        match self.config.get() {
            Some(Some(sc)) => sc.category(meta.target()).is_some(),
            _ => false,
        }
    }
}

impl<S: Subscriber> Filter<S> for SamplingFilter {
    fn enabled(&self, meta: &Metadata<'_>, _: &Context<'_, S>) -> bool {
        self.is_enabled(meta)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        if self.is_enabled(meta) {
            Interest::always()
        } else {
            Interest::never()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    use super::*;

    static TEST_CONFIG: OnceLock<Option<Arc<SamplingConfig>>> = OnceLock::new();

    #[test]
    fn test_sampling_layer() {
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("out");

        {
            // Config not initialized yet.
            let sl = SamplingLayer::new_with_config(&TEST_CONFIG);

            let _subscriber = tracing::subscriber::set_default(Registry::default().with(sl));

            // layer not configured, shouldn't show up.
            tracing::info!(target: "banana", foo = "bar");

            let filepath = out_path.to_string_lossy();
            let config = BTreeMap::<&str, &str>::from([
                ("sampling.filepath", filepath.as_ref()),
                ("sampling.key.banana", "pear"),
                ("sampling.key.orange", "melon"),
            ]);

            TEST_CONFIG
                .set(Some(Arc::new(SamplingConfig::new(&config).unwrap())))
                .unwrap();

            // Should be picked up.
            tracing::info!(target: "banana", foo = "baz");

            // Target isn't sampled.
            tracing::info!(target: "apple", foo = "baz");

            // Should also be collected.
            tracing::info!(target: "orange", pi = 123);
        }

        assert_eq!(
            std::fs::read(&out_path).unwrap(),
            b"{\"category\":\"pear\",\"data\":{\"foo\":\"baz\"}}\0{\"category\":\"melon\",\"data\":{\"pi\":123}}\0",
        );
    }
}
