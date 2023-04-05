/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::path::PathBuf;

use parking_lot::Mutex;
use parking_lot::MutexGuard;

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
                .write(true)
                .append(true)
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

    pub fn category(&self, key: &str) -> Option<&str> {
        self.keys.get(key).map(|c| &**c)
    }

    pub fn file(&self) -> MutexGuard<File> {
        self.file.lock()
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
