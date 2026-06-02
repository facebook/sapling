/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// [cats]
// entry_name.priority=20
// entry_name.path=/var/boo/cat
// entry_name.type=forwarded  # If not present, "forwarded" is the default.
// different_entry_name.priority=5
// different_entry_name.path=/some/other
// different_entry_name.type=auth
// different_entry_name.wanted-key=scm_service_identity
//
// Forwarded and auth types are completely orthogonal. Each type is
// resolved independently to the highest-priority group of that type.
// The same token file can appear in groups of both types, causing it
// to be sent in both x-forwarded-cats and x-auth-cats headers.
//
// When wanted-key is set, the JSON file is read as a map
// and only the value at that key is sent. Without it, the default
// "crypto_auth_tokens" key is used.

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

#[derive(Deserialize)]
struct PremintedCats {
    crypto_auth_tokens: String,
    #[serde(flatten)]
    extra: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatTokenType {
    Forwarded,
    Auth,
}

impl CatTokenType {
    pub fn from_type_str(s: &str) -> Result<Self> {
        match s {
            "forwarded" => Ok(Self::Forwarded),
            "auth" => Ok(Self::Auth),
            other => anyhow::bail!("unknown CAT token type: {other}"),
        }
    }

    pub fn header_name(&self) -> &'static str {
        match self {
            Self::Forwarded => cats_constants::X_FORWARDED_CATS_HEADER,
            Self::Auth => cats_constants::X_AUTH_CATS_HEADER,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatGroup {
    pub name: String,
    pub priority: i32,
    pub path: Option<PathBuf>,
    pub token_type: CatTokenType,
    #[serde(default)]
    pub wanted_key: Option<String>,
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

        let token_type = settings
            .remove("type")
            .map(|s| CatTokenType::from_type_str(&s))
            .transpose()?
            .unwrap_or(CatTokenType::Forwarded);

        let wanted_key = settings.remove("wanted-key").map(|s| s.trim().to_string());

        Ok(Self {
            name,
            priority,
            path,
            token_type,
            wanted_key,
        })
    }
}

#[derive(Clone)]
pub struct CatsSection {
    groups: Vec<CatGroup>,
}

impl CatsSection {
    pub fn from_config(config: &dyn Config, section_name: &str) -> Self {
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

        Self { groups }
    }

    /// Find existing cats with highest priority, filtered by token type.
    pub fn find_cats_by_type(
        &self,
        token_type: CatTokenType,
    ) -> Result<Option<CatGroup>, MissingCATs> {
        let mut best: Option<&CatGroup> = None;
        let mut missing = Vec::new();

        for group in self.groups.iter().filter(|g| g.token_type == token_type) {
            // If there is an existing candidate, check whether the current
            // cats entry is a more specific match.
            if let Some(best) = best {
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

    pub fn get_cats_by_type(&self, token_type: CatTokenType) -> Result<Option<String>> {
        if let Some(cats_group) = self.find_cats_by_type(token_type)? {
            if let Some(path) = cats_group.path {
                let f = std::fs::File::open(&path)?;
                let reader = std::io::BufReader::new(f);
                let preminted: PremintedCats = serde_json::from_reader(reader)?;
                let token = match cats_group.wanted_key.as_deref() {
                    Some(key) => match preminted.extra.get(key) {
                        Some(value) => value.clone(),
                        None => {
                            tracing::warn!(
                                "[cats] group {:?}: wanted-key {:?} not found in {:?}, falling back to crypto_auth_tokens",
                                &cats_group.name,
                                key,
                                &path,
                            );
                            preminted.crypto_auth_tokens
                        }
                    },
                    None => preminted.crypto_auth_tokens,
                };
                return Ok(Some(token));
            }
        }

        Ok(None)
    }
}
