/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::str::FromStr;

use anyhow::Context;
use blake3::Hasher as Blake3Hasher;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use types::Blake3;
use types::HgId;
use types::RepoPathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
pub enum FilterVersion {
    /// Legacy Filters could only support having a single active filter. The filter content was
    /// stored inside the FilterID itself.
    Legacy = 0,
    /// V1 filters support multiple active filters. The filter content is stored on disk in an
    /// indexedlog where the partial Blake3 hash of the filter content is used to access the log
    /// entries. V1 Filters are in the form:
    ///
    /// - List of Filter Paths that should be used to construct a sparse matcher
    /// - HgId of the commit that the filter was activated at
    /// - Id of the filter, which contains the FilterVersion and the first 8 bytes of the
    ///   Filter's Blake3 hash which is used as an index for filter storage.
    V1 = 1,
}

impl Serialize for FilterVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            FilterVersion::Legacy => Err(serde::ser::Error::custom(
                "Serializing Legacy FilterVersions is not permitted",
            )),
            FilterVersion::V1 => serializer.serialize_u8(self.clone() as u8),
        }
    }
}

impl<'de> Deserialize<'de> for FilterVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = u8::deserialize(deserializer)?;
        match s {
            0 => Err(serde::de::Error::custom(
                "Deserializing Legacy FilterVersions is not permitted",
            )),
            1 => Ok(FilterVersion::V1),
            v => Err(serde::de::Error::custom(format!(
                "Unknown filter version: {}",
                v
            ))),
        }
    }
}

impl FromStr for FilterVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "V1" => Ok(FilterVersion::V1),
            "Legacy" => Ok(FilterVersion::Legacy),
            _ => Err(anyhow::anyhow!("Invalid FilterVersion: {}", s)),
        }
    }
}

#[derive(Debug, Serialize)]
struct V1FilterComponents<'a> {
    version: FilterVersion,
    filter_paths: &'a [RepoPathBuf],
    commit_id: &'a HgId,
}

// Eden's ObjectIDs must be durable (once they exist, Eden must always be able to derive
// the underlying object from them). FilteredObjectIDs contain FilterIDs, and therefore we
// must be able to re-derive the filter contents from any FilterID so that we can properly
// reconstruct the original filtered object at any future point in time. To do this, we
// associate a commit ID to each Filter Path which allows us to read Filter file contents from
// the repo and reconstruct any filtered object.
#[derive(Debug, Serialize, Deserialize)]
pub enum FilterId {
    /// Legacy FilterIDs are in the form:
    ///
    /// [filter_file_paths]:[hex_commit_hash]
    ///
    /// Where the filter_file_path indicates the path to the filter file relative to the repo root,
    /// and commit_hash indicates the version of the filter file when it was applied. This format
    /// assumes that neither the filter files nor the commit hash will have ":" in them. The second
    /// restriction is guaranteed (hex), the first one needs to be enforced by us.
    #[serde(skip_serializing)]
    Legacy(Vec<u8>),
    /// V1 Filter IDs contain:
    ///
    /// - FilterID Version
    /// - Partial Blake3 Hash (index)
    ///
    /// Where the version is "V1" and the hash is the first 8 bytes of the Filter's Blake3 hash.
    ///
    /// Filter IDs are serialized with mincode::serialize when they are passed to EdenFS. When used
    /// as an index in the Filter IndexedLog, only the 8 byte Blake3 hash of the filter is used.
    V1(FilterVersion, Vec<u8>),
}

impl FilterId {
    pub fn id(&self) -> anyhow::Result<Vec<u8>> {
        match self {
            FilterId::Legacy(id) => Ok(id.clone()),
            FilterId::V1(_, _) => {
                mincode::serialize(self).with_context(|| anyhow::anyhow!("Serialization failed"))
            }
        }
    }

    // TODO(cuev): Strongly type the index after Legacy filters are removed
    pub fn index(&self) -> &[u8] {
        match self {
            FilterId::Legacy(id) => id.as_ref(),
            FilterId::V1(_, index) => index.as_ref(),
        }
    }

    #[allow(dead_code)]
    pub fn version(&self) -> FilterVersion {
        match self {
            FilterId::Legacy(_) => FilterVersion::Legacy,
            FilterId::V1(_, _) => FilterVersion::V1,
        }
    }

    pub fn from_bytes(b: &[u8]) -> anyhow::Result<Self> {
        match mincode::deserialize(b) {
            Ok(filter) => Ok(filter),
            Err(_) => {
                let filter = str::from_utf8(b)?;
                let parts = filter.split(":");
                if parts.count() != 2 {
                    Err(anyhow::anyhow!("Unknown filter id type: {:?}", b))
                } else {
                    Ok(FilterId::Legacy(b.to_vec()))
                }
            }
        }
    }
}

impl FilterId {
    pub(crate) fn new(
        version: FilterVersion,
        filter_paths: &[RepoPathBuf],
        commit_id: &HgId,
        hash_key: &[u8; 32],
    ) -> Result<FilterId, anyhow::Error> {
        match version {
            FilterVersion::Legacy => {
                if filter_paths.len() != 1 {
                    Err(anyhow::anyhow!(
                        "Legacy FilterIDs must only contain a single filter path"
                    ))
                } else {
                    let id = format!("{}:{}", filter_paths[0], commit_id);
                    Ok(FilterId::Legacy(id.into()))
                }
            }
            FilterVersion::V1 => {
                let v1_components = V1FilterComponents {
                    version,
                    filter_paths,
                    commit_id,
                };

                // Create a hash out of the serialized V1 filter components.
                let mut hasher = Blake3Hasher::new_keyed(hash_key);
                let filter_bytes = mincode::serialize(&v1_components)?;
                hasher.update(&filter_bytes);
                let index = hasher.finalize();

                Ok(FilterId::V1(
                    FilterVersion::V1,
                    index.as_bytes()[..Blake3::len() / 4].into(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mincode::deserialize;
    use mincode::serialize_into;

    use super::*;

    // 40-character hex commit ID for tests
    const TEST_COMMIT_ID: &[u8] = b"1234567890123456789012345678901234567890";
    const TEST_COMMIT_ID_STR: &str = "1234567890123456789012345678901234567890";

    const DEFAULT_FILTER_PATH: &str = "path/to/filter.txt";

    #[test]
    fn test_filter_version_serialize() {
        let mut buffer = Vec::new();
        serialize_into(&mut buffer, &FilterVersion::V1).unwrap();
        assert!(!buffer.is_empty());
        // mincode serializer prefixes strings with their length (VLQ encoded)
        assert_eq!(buffer, vec![1]);

        let mut buffer = Vec::new();
        let res = serialize_into(&mut buffer, &FilterVersion::Legacy);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("permitted"));
    }

    #[test]
    fn test_filter_version_deserialize() {
        let v1_bytes = vec![1];
        let version: FilterVersion = deserialize(&v1_bytes).unwrap();
        assert_eq!(version, FilterVersion::V1);

        let legacy_bytes = vec![0];
        let result: Result<FilterVersion, _> = deserialize(&legacy_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("permitted"));

        let unknown_bytes = vec![7, b'U', b'n', b'k', b'n', b'o', b'w', b'n'];
        let result: Result<FilterVersion, _> = deserialize(&unknown_bytes);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown filter version: 7")
        );
    }

    #[test]
    fn test_filter_version_round_trip() {
        let original_v1 = FilterVersion::V1;
        let mut buffer = Vec::new();
        serialize_into(&mut buffer, &original_v1).unwrap();
        let deserialized: FilterVersion = deserialize(&buffer).unwrap();
        assert_eq!(original_v1, deserialized);
    }

    #[test]
    fn test_filter_id_legacy_creation() {
        let filter_paths = vec![RepoPathBuf::from_utf8(DEFAULT_FILTER_PATH.into()).unwrap()];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let filter_id =
            FilterId::new(FilterVersion::Legacy, &filter_paths, &commit_id, &[0; 32]).unwrap();

        if let FilterId::Legacy(id) = filter_id {
            assert_eq!(
                id,
                format!("{}:{}", DEFAULT_FILTER_PATH, TEST_COMMIT_ID_STR).as_bytes()
            );
        } else {
            panic!("Expected Legacy FilterId");
        }
    }

    #[test]
    fn test_filter_id_legacy_multiple_paths_error() {
        let filter_paths = vec![
            RepoPathBuf::from_utf8("path1.txt".into()).unwrap(),
            RepoPathBuf::from_utf8("path2.txt".into()).unwrap(),
        ];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let result = FilterId::new(FilterVersion::Legacy, &filter_paths, &commit_id, &[0; 32]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Legacy FilterIDs must only contain a single filter path")
        );
    }

    #[test]
    fn test_filter_id_v1_creation() {
        let filter_paths = vec![RepoPathBuf::from_utf8(DEFAULT_FILTER_PATH.into()).unwrap()];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();
        let hash_key = [42u8; 32];

        let filter_id =
            FilterId::new(FilterVersion::V1, &filter_paths, &commit_id, &hash_key).unwrap();

        if let FilterId::V1(_, index) = filter_id {
            // ID Should be 8 bytes long
            assert!(index.len() == (Blake3::len() / 4));
            assert_eq!(index, [160, 95, 149, 78, 3, 46, 174, 41]);
        } else {
            panic!("Expected V1 FilterId");
        }
    }
}
