/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// [cats]
// entry_name.priority=20
// entry_name.path=/var/boo/cat
// different_entry_name.priority=5
// different_entry_name.more_custom_data=/some/other

use std::collections::HashMap;
use std::path::PathBuf;
use std::str;

use anyhow::Result;
use configmodel::Config;
use configmodel::Text;
use indexmap::IndexMap;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;
use util::path::expand_path;

#[derive(Debug, Error)]
#[error("File containing CATs not found in {missing:?}.")]
pub struct MissingCATs {
    missing: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Cats {
    crypto_auth_tokens: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatGroup {
    pub name: String,
    pub priority: i32,
    pub path: Option<PathBuf>,
}

impl CatGroup {
    fn new(group: &str, mut settings: HashMap<&str, Text>) -> Result<Self> {
        let name = group.into();

        let path = settings
            .remove("path")
            .filter(|s| !s.is_empty())
            .map(expand_path);

        let priority = settings
            .remove("priority")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            name,
            priority,
            path,
        })
    }
}

#[derive(Clone)]
pub struct CatsSection<'a> {
    groups: Vec<CatGroup>,
    #[allow(dead_code)]
    config: &'a dyn Config,
}

impl<'a> CatsSection<'a> {
    pub fn from_config(config: &'a dyn Config, section_name: &str) -> Self {
        // Use an IndexMap to preserve ordering; needed to correctly handle precedence.
        let mut groups = IndexMap::new();

        let keys = config.keys(section_name);
        for key in &keys {
            // Skip keys that aren't valid UTF-8 or that don't match
            // the expected cats key format of `group.setting`.
            let (group, setting) = match key.find('.') {
                Some(i) => (&key[..i], &key[i + 1..]),
                None => continue,
            };
            if let Some(value) = config.get(section_name, key) {
                groups
                    .entry(group)
                    .or_insert_with(HashMap::new)
                    .insert(setting, value);
            }
        }

        let groups = groups
            .into_iter()
            .filter_map(|(group, settings)| CatGroup::new(group, settings).ok())
            .collect();

        Self { groups, config }
    }

    /// Find existing cats with highest priority.
    pub fn find_cats(&self) -> Result<Option<CatGroup>, MissingCATs> {
        let mut best: Option<&CatGroup> = None;
        let mut missing = Vec::new();

        for group in &self.groups {
            // If there is an existing candidate, check whether the current
            // cats entry is a more specific match.
            if let Some(ref best) = best {
                // If prefixes are the same, break the tie using priority.
                if group.priority < best.priority {
                    continue;
                }
            }

            // Skip this group is any of the files are missing.
            match &group.path {
                Some(path) if !path.is_file() => {
                    tracing::debug!(
                        "Ignoring [cats] group {:?} because of missing {:?}",
                        &group.name,
                        &path
                    );
                    missing.push(path.to_path_buf());
                    continue;
                }
                _ => {}
            }

            best = Some(group);
        }

        if let Some(best) = best {
            Ok(Some(best.clone()))
        } else if !missing.is_empty() {
            Err(MissingCATs { missing })
        } else {
            Ok(None)
        }
    }

    pub fn get_cats(&self) -> Result<Option<String>> {
        if let Some(cats_group) = self.find_cats()? {
            if let Some(path) = cats_group.path {
                let f = std::fs::File::open(path)?;
                let reader = std::io::BufReader::new(f);

                let cats: Cats = serde_json::from_reader(reader)?;
                let cats_data = cats.crypto_auth_tokens;

                return Ok(Some(cats_data));
            }
        }
        Ok(None)
    }
}
