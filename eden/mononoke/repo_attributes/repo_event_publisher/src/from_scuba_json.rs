/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! From Scuba JSON.
//!
//! Trait outlining the interface for generating value out of json scuba data.

#[cfg(fbcode_build)]
use anyhow::Context;
#[cfg(fbcode_build)]
use mononoke_bookmark_event_notification::BookmarkCategory;
#[cfg(fbcode_build)]
use mononoke_bookmark_event_notification::BookmarkEventNotification;
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

#[cfg(fbcode_build)]
impl FromScubaJson for BookmarkEventNotification {
    type Output = Self;

    fn from_scuba_json(json: serde_json::Value) -> anyhow::Result<Self> {
        let bookmark_category = match get_normal(&json, "bookmark_category")?.as_str() {
            "BRANCH" => BookmarkCategory::BRANCH,
            "TAG" => BookmarkCategory::TAG,
            "NOTE" => BookmarkCategory::NOTE,
            value => BookmarkCategory::from(
                value
                    .parse::<i32>()
                    .context("bookmark_category is not valid")?,
            ),
        };

        Ok(Self {
            repo_id: get_normal(&json, "repo_id")?
                .parse()
                .context("repo_id is not valid")?,
            log_id: get_normal(&json, "log_id")?
                .parse()
                .context("log_id is not valid")?,
            bookmark_name: get_normal(&json, "bookmark_name")?,
            bookmark_category,
            ..Default::default()
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

#[cfg(all(test, fbcode_build))]
mod tests {
    use anyhow::Result;
    use mononoke_macros::mononoke;
    use serde_json::json;

    use super::*;

    #[mononoke::test]
    fn deserializes_bookmark_event_notification() -> Result<()> {
        let deserialized = BookmarkEventNotification::from_scuba_json(json!({
            "normal": {
                "repo_id": "123",
                "log_id": "456",
                "bookmark_name": "main",
                "bookmark_category": "BRANCH",
            },
        }))?;

        assert_eq!(deserialized.repo_id, 123);
        assert_eq!(deserialized.log_id, 456);
        assert_eq!(deserialized.bookmark_name, "main");
        assert_eq!(deserialized.bookmark_category, BookmarkCategory::BRANCH);
        Ok(())
    }
}
