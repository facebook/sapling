/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! From Scuba JSON.
//!
//! Trait outlining the interface for generating value out of json scuba data.

use repo_update_logger::GitContentRefInfo;
use repo_update_logger::PlainBookmarkInfo;

/// Trait outlining the interface for generating value out of json scuba data
pub trait FromScubaJson {
    type Output;
    /// Get the name of the repo.
    fn from_scuba_json(json: serde_json::Value) -> anyhow::Result<Self::Output>;
}

impl FromScubaJson for PlainBookmarkInfo {
    type Output = Self;

    fn from_scuba_json(json: serde_json::Value) -> anyhow::Result<Self> {
        let bookmark_name = get_normal(&json, "bookmark_name")?;
        let bookmark_kind = get_normal(&json, "bookmark_kind")?;
        let old_bookmark_value = maybe_get_normal(&json, "old_bookmark_value");
        let new_bookmark_value = maybe_get_normal(&json, "new_bookmark_value");
        let repo_name = get_normal(&json, "repo_name")?;
        let operation = get_normal(&json, "operation")?;
        let update_reason = get_normal(&json, "update_reason")?;
        Ok(Self {
            bookmark_name,
            bookmark_kind,
            old_bookmark_value,
            new_bookmark_value,
            repo_name,
            operation,
            update_reason,
        })
    }
}

impl FromScubaJson for GitContentRefInfo {
    type Output = Self;

    fn from_scuba_json(json: serde_json::Value) -> anyhow::Result<Self> {
        let repo_name = get_normal(&json, "repo_name")?;
        let ref_name = get_normal(&json, "ref_name")?;
        let git_hash = get_normal(&json, "git_hash")?;
        let object_type = get_normal(&json, "object_type")?;
        Ok(Self {
            repo_name,
            ref_name,
            git_hash,
            object_type,
        })
    }
}

fn get_normal(val: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    maybe_get_normal(val, key).ok_or_else(|| anyhow::anyhow!("{key} not found"))
}

fn maybe_get_normal(val: &serde_json::Value, key: &str) -> Option<String> {
    val["normal"][key]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| val[key].as_str().map(|s| s.to_string()))
}
