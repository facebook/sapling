/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

pub use clientinfo::get_client_request_info;
use parking_lot::Mutex;
use parking_lot::MutexGuard;
use serde::Serializer;
use serde::ser::Serialize;
use serde::ser::SerializeMap;
pub use serde_json;
use serde_json::Serializer as JsonSerializer;

pub static CONFIG: OnceLock<Option<Arc<SamplingConfig>>> = OnceLock::new();

pub fn init(config: &dyn configmodel::Config) {
    CONFIG.get_or_init(|| SamplingConfig::new(config).map(Arc::new));
}

pub fn flush() {
    if let Some(Some(sc)) = CONFIG.get() {
        let _ = sc.file().flush();
    }
}

/// Log a single key->value pair.
pub fn append_sample<V>(key: &str, name: &str, value: &V)
where
    V: ?Sized + Serialize,
{
    append_sample_map(key, &HashMap::from([(name, value)]));
}

/// Log a key->value map of some kind. `value` should serialize to a JSON object.
pub fn append_sample_map<V>(key: &str, value: &V)
where
    V: ?Sized + Serialize,
{
    if let Some(Some(sc)) = CONFIG.get() {
        let category = match sc.category(key) {
            Some(v) => v,
            None => return,
        };
        let _ = sc.append(category, value);
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

        if let Some((output_file, okay_exists)) = sampling_output_file(config) {
            match OpenOptions::new()
                .create(okay_exists)
                .create_new(!okay_exists)
                .append(true)
                .open(&output_file)
            {
                Ok(file) => {
                    return Some(Self {
                        keys: sample_categories,
                        file: Mutex::new(file),
                    });
                }
                Err(err) => {
                    // This is expected for child commands that skirt the telemetry wrapper.
                    tracing::warn!(
                        ?err,
                        ?output_file,
                        "error opening sampling file (expected for child commands)"
                    );
                }
            }
        }

        None
    }

    pub fn category(&self, key: &str) -> Option<&str> {
        self.keys.get(key).map(|c| &**c)
    }

    pub fn file(&self) -> MutexGuard<'_, File> {
        self.file.lock()
    }

    pub fn append<V>(&self, category: &str, value: &V) -> std::io::Result<()>
    where
        V: ?Sized + Serialize,
    {
        let mut file = self.file();
        let mut serializer = JsonSerializer::new(&*file);

        let mut serializer = serializer.serialize_map(None)?;
        serializer.serialize_entry("category", category)?;
        serializer.serialize_entry("data", value)?;
        serializer.end()?;

        file.write_all(b"\0")?;

        Ok(())
    }
}

/// Similar to `tracing::info!(target: $target, $key = $value, ...)`, but `$value`
/// can be any serde type, not just tracing's limited `Value`.
#[macro_export]
macro_rules! log {
    (target: $target:expr $(, $key:ident = $value:expr)*) => {
        'block: {
            if let Some(Some(config)) = $crate::CONFIG.get() {
                if let Some(category) = config.category($target) {
                    break 'block config.append(category, &$crate::serde_json::json!({$(stringify!($key): $value),*}));
                }
            }
            Ok(())
        }
    };
}

/// Log an event to the `sl_events` tracing target.
#[macro_export]
macro_rules! log_event {
    ($event_type:expr $(, $key:ident = $value:expr )*) => {
        let correlator = $crate::get_client_request_info().correlator;

        tracing::info!(
            target: "sl_events",
            client_correlator=correlator,
            event_type=$event_type,
            event_value=$crate::serde_json::json!({$(stringify!($key): $value),*}).to_string(),
        );
    }
}

// Returns tuple of output path and whether it's okay if the path already exists.
fn sampling_output_file(config: &dyn configmodel::Config) -> Option<(PathBuf, bool)> {
    let mut candidates: Vec<(PathBuf, bool)> = Vec::with_capacity(2);

    if let Ok(path) = std::env::var("SCM_SAMPLING_FILEPATH") {
        // Env var is not-okay-exists (i.e. only one process should respect this).
        candidates.push((path.into(), false));
    }

    if let Some(path) = config.get("sampling", "filepath") {
        // Config setting is okay to be shared across multiple commands (mainly
        // for test compat).
        candidates.push((path.to_string().into(), true));
    }

    candidates
        .into_iter()
        .find(|(path, _okay_exists)| path.parent().is_some_and(|d| d.exists()))
}
