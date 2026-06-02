/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Computes the `ConfigInfo` metadata (deterministic content hash + last-updated
//! timestamp) for a `RawRepoConfigs` blob.
//!
//! The content hash sorts thrift struct keys before hashing so that equivalent
//! configs differing only in serialization key order produce identical hashes.
//! This makes the hash safe to use as a cache key across config refresh cycles.

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use metaconfig_types::ConfigInfo;
use repos::RawRepoConfigs;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

/// Computes the content-hash + last-updated-at pair used by `MononokeConfigs`
/// to expose stable identity for a `RawRepoConfigs` snapshot via
/// `MononokeConfigs::config_info`.
pub(crate) fn build_config_info(raw_repo_configs: Arc<RawRepoConfigs>) -> Result<ConfigInfo> {
    let content_hash = {
        let serialized = serde_json::to_string(&SortKeys(raw_repo_configs))
            .expect("RawRepoConfigs serialization should never fail");
        let mut hasher = Sha256::new();
        hasher.update(serialized);
        let hash = hasher.finalize();
        hex::encode(hash)
    };

    let last_updated_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("system clock before UNIX epoch")?
        .as_secs();

    Ok(ConfigInfo {
        content_hash,
        last_updated_at,
    })
}

/// Wrapper that forces serialization through `serde_json::to_value` first,
/// which sorts map keys lexicographically. Without this, two equivalent
/// configs serialized by thrift's HashMap iteration could produce different
/// hashes purely from insertion order.
#[derive(Serialize)]
struct SortKeys<T: Serialize>(#[serde(serialize_with = "serialize_to_value")] T);

fn serialize_to_value<T: Serialize, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
    value.serialize(serializer)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mononoke_macros::mononoke;
    use repos::RawRepoConfig;

    use super::*;

    #[mononoke::test]
    fn test_build_config_info_empty() {
        let results = (1..10)
            .map(|_i| {
                let raw_repo_configs = RawRepoConfigs::default();
                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    #[mononoke::test]
    fn test_build_config_info_one_repo() {
        let results = (1..10)
            .map(|_| {
                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    #[mononoke::test]
    fn test_build_config_info_two_repos() {
        let results = (1..10)
            .flat_map(|_| {
                let mut ret = Vec::new();

                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());
                raw_repo_configs
                    .repos
                    .insert("repo2".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);
                ret.push(info.content_hash);

                // Test that the hash is identical if the order of the repos is different
                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo2".to_string(), RawRepoConfig::default());
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);
                ret.push(info.content_hash);

                ret
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    // The smallest fixture that did *not* demonstrate non-deterministic behavior
    // with the old implementation.
    #[mononoke::test]
    fn test_build_config_info_minimal() {
        let results = (1..10)
            .map(|_| {
                let json = fixtures::json_config_minimal();
                let raw_repo_configs =
                    serde_json::from_str::<RawRepoConfigs>(&json).expect("Unable to parse");

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });

        assert_eq!(results.len(), 1);
    }

    // The smallest fixture that *did* demonstrate non-deterministic behavior
    // with the old implementation.
    #[mononoke::test]
    fn test_build_config_info_small() {
        let results = (1..10)
            .map(|_| {
                let json = fixtures::json_config_small();
                let raw_repo_configs =
                    serde_json::from_str::<RawRepoConfigs>(&json).expect("Unable to parse");

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }
}
