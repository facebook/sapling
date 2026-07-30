/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::mem::size_of;
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

const PENDING_BYTE_LIMIT: usize = 8 * 1024 * 1024;
const PENDING_ENTRY_LIMIT: usize = 4096;

pub static CONFIG: OnceLock<Option<Arc<SamplingConfig>>> = OnceLock::new();
static PENDING: Mutex<PendingSamples> = Mutex::new(PendingSamples {
    samples: Vec::new(),
    bytes: 0,
});

pub fn init(config: &dyn configmodel::Config) {
    if CONFIG.get().is_some() {
        return;
    }

    let mut pending = PENDING.lock();
    if CONFIG.get().is_some() {
        return;
    }

    let sampling_config = SamplingConfig::new(config).map(Arc::new);
    let _ = CONFIG.set(sampling_config);
    let samples = std::mem::take(&mut pending.samples);
    pending.bytes = 0;
    drop(pending);

    // New events can overtake this batch, but buffered events stay FIFO and
    // none can be stranded between publishing CONFIG and detaching the batch.
    let sampling_config = CONFIG.get().and_then(Option::as_deref);
    for sample in samples {
        if let (Some(config), Ok(value)) = (
            sampling_config,
            serde_json::from_slice::<&serde_json::value::RawValue>(&sample.value),
        ) {
            let _ = config.append_key(&sample.key, value);
        }
    }
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

struct PendingSample {
    key: Box<str>,
    value: Box<[u8]>,
}

#[derive(Default)]
struct PendingSamples {
    samples: Vec<PendingSample>,
    bytes: usize,
}

impl PendingSamples {
    fn push(&mut self, key: &str, value: &serde_json::Value) {
        if self.samples.len() >= PENDING_ENTRY_LIMIT {
            return;
        }
        let overhead = size_of::<PendingSample>() + key.len();
        let remaining = PENDING_BYTE_LIMIT.saturating_sub(self.bytes.saturating_add(overhead));
        if remaining == 0 {
            return;
        }
        let Ok(value) = serde_json::to_vec(value) else {
            return;
        };
        if value.len() > remaining {
            return;
        }
        let sample = PendingSample {
            key: key.into(),
            value: value.into(),
        };
        self.bytes += overhead + sample.value.len();
        self.samples.push(sample);
    }
}

#[derive(Debug)]
pub struct SamplingConfig {
    keys: HashMap<String, String>,
    file: Mutex<BufWriter<File>>,
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
                        file: Mutex::new(BufWriter::new(file)),
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

    fn append_key<V: ?Sized + Serialize>(&self, key: &str, value: &V) -> io::Result<()> {
        match self.category(key) {
            Some(category) => self.append(category, value),
            None => Ok(()),
        }
    }

    pub fn file(&self) -> MutexGuard<'_, BufWriter<File>> {
        self.file.lock()
    }

    pub fn append<V>(&self, category: &str, value: &V) -> std::io::Result<()>
    where
        V: ?Sized + Serialize,
    {
        let mut file = self.file();
        let mut serializer = JsonSerializer::new(&mut *file);

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
/// Before initialization, values are evaluated for bounded buffering; keep them cheap and side-effect-free.
#[macro_export]
macro_rules! log {
    (target: $target:expr $(, $key:ident = $value:expr)*) => {
        match $crate::CONFIG.get() {
            Some(Some(config)) => match config.category($target) {
                Some(category) => config.append(category, &$crate::serde_json::json!({$(stringify!($key): $value),*})),
                None => Ok(()),
            },
            Some(None) => Ok(()),
            None => $crate::log_before_init(
                $target,
                $crate::serde_json::json!({$(stringify!($key): $value),*}),
            ),
        }
    };
}

#[doc(hidden)]
pub fn log_before_init(key: &str, value: serde_json::Value) -> io::Result<()> {
    let mut pending = PENDING.lock();
    if let Some(config) = CONFIG.get() {
        let config = config.clone();
        drop(pending);
        return match config {
            Some(config) => config.append_key(key, &value),
            None => Ok(()),
        };
    }
    pending.push(key, &value);
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Barrier;

    use super::*;

    #[test]
    fn log_does_not_lose_events_during_init() {
        let path = std::env::temp_dir().join(format!("sampling-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let config = BTreeMap::from([
            ("sampling.filepath".to_owned(), path.display().to_string()),
            ("sampling.key.banana".to_owned(), "pear".to_owned()),
        ]);

        crate::log!(target: "banana", value = 1).unwrap();
        crate::log!(target: "apple", value = 0).unwrap();
        crate::log!(target: "banana", value = 2).unwrap();

        let serializing = Barrier::new(2);
        let resume = Barrier::new(2);
        std::thread::scope(|scope| {
            let log = scope.spawn(|| {
                crate::log!(
                    target: "banana",
                    value = {
                        serializing.wait();
                        resume.wait();
                        3
                    }
                )
            });
            serializing.wait();
            init(&config);
            resume.wait();
            log.join().unwrap().unwrap();
        });

        crate::log!(target: "banana", value = 4).unwrap();
        let mut evaluated = false;
        crate::log!(target: "apple", value = {
            evaluated = true;
            false
        })
        .unwrap();
        assert!(!evaluated);
        flush();

        let samples = std::fs::read(&path).unwrap();
        let values: Vec<u64> = samples
            .split(|byte| *byte == 0)
            .filter(|sample| !sample.is_empty())
            .map(|sample| {
                serde_json::from_slice::<serde_json::Value>(sample).unwrap()["data"]["value"]
                    .as_u64()
                    .unwrap()
            })
            .collect();
        let mut sorted = values.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3, 4]);
        assert!(
            values.iter().position(|value| *value == 1)
                < values.iter().position(|value| *value == 2)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pending_samples_are_bounded() {
        let mut pending = PendingSamples::default();
        pending.push(
            "key",
            &serde_json::Value::String("x".repeat(PENDING_BYTE_LIMIT)),
        );
        assert!(pending.samples.is_empty());

        pending.push("key", &serde_json::json!(1));
        assert_eq!(pending.samples.len(), 1);
        assert_eq!(pending.bytes, size_of::<PendingSample>() + 4);

        for _ in 1..=PENDING_ENTRY_LIMIT {
            pending.push("key", &serde_json::json!(1));
        }
        assert_eq!(pending.samples.len(), PENDING_ENTRY_LIMIT);
    }
}
