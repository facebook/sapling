/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::ser::SerializeMap;
use serde::ser::Serializer;
use serde_json::Serializer as JsonSerializer;
use tracing::Event;
use tracing::Subscriber;
use tracing_serde::fields::AsMap;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub struct SamplingLayer {
    config: Arc<OnceCell<SamplingConfig>>,
}

impl SamplingLayer {
    pub fn new(config: Arc<OnceCell<SamplingConfig>>) -> Self {
        Self { config }
    }
}

impl<S: Subscriber> Layer<S> for SamplingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let config = match self.config.get() {
            Some(config) => config,
            None => return,
        };

        let category = match config.keys.get(event.metadata().target()) {
            Some(v) => v,
            None => return,
        };

        let serialize = || -> std::io::Result<()> {
            let mut file = config.file.lock();
            let mut serializer = JsonSerializer::new(&*file);

            let mut serializer = serializer.serialize_map(None)?;
            serializer.serialize_entry("category", category)?;
            serializer.serialize_entry("data", &event.field_map())?;
            serializer.end()?;

            file.write_all(b"\0")?;

            Ok(())
        };

        let _ = serialize();
    }
}

#[derive(Debug)]
pub struct SamplingConfig {
    keys: HashMap<String, String>,
    file: Mutex<File>,
}

impl SamplingConfig {
    pub fn new(config: &dyn configmodel::Config) -> Option<Self> {
        let sample_categories: HashMap<String, String> = config
            .keys("sampling")
            .into_iter()
            .filter_map(|name| {
                if let Some(key) = name.strip_prefix("key.") {
                    if let Some(val) = config.get("sampling", &name) {
                        return Some((key.to_string(), val.to_string()));
                    }
                }
                None
            })
            .collect();
        if sample_categories.is_empty() {
            return None;
        }


        if let Some(output_file) = sampling_output_file(config) {
            if let Ok(file) = OpenOptions::new()
                .create(true)
                .append(true)
                .write(true)
                .open(output_file)
            {
                return Some(Self {
                    keys: sample_categories,
                    file: Mutex::new(file),
                });
            }
        }

        None
    }
}

fn sampling_output_file(config: &dyn configmodel::Config) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::with_capacity(2);

    if let Ok(path) = std::env::var("SCM_SAMPLING_FILEPATH") {
        candidates.push(path.into());
    }

    if let Some(path) = config.get("sampling", "filepath") {
        candidates.push(path.to_string().into());
    }

    candidates
        .into_iter()
        .find(|path| path.parent().map_or(false, |d| d.exists()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    use super::*;

    #[test]
    fn test_sampling_layer() {
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("out");

        {
            // Config not initialized yet.
            let sc = Arc::new(OnceCell::<SamplingConfig>::new());
            let sl = SamplingLayer::new(sc.clone());

            let _subscriber = tracing::subscriber::set_default(Registry::default().with(sl));

            // layer not configured, shouldn't show up.
            tracing::info!(target: "banana", foo = "bar");


            let config = BTreeMap::<String, String>::from([
                (
                    "sampling.filepath".to_string(),
                    out_path.to_string_lossy().to_string(),
                ),
                ("sampling.key.banana".to_string(), "pear".to_string()),
                ("sampling.key.orange".to_string(), "melon".to_string()),
            ]);

            sc.set(SamplingConfig::new(&config).unwrap()).unwrap();

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
