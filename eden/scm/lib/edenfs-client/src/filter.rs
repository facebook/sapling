/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use anyhow::Context;
use blake3::Hasher as Blake3Hasher;
use indexedlog::log::IndexOutput;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use tracing::warn;
use types::Blake3;
use types::HgId;
use types::RepoPathBuf;
use types::sha::to_hex;

#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(dead_code)]
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

impl fmt::Display for FilterVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilterVersion::V1 => write!(f, "V1"),
            FilterVersion::Legacy => write!(f, "Legacy"),
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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

    pub fn version(&self) -> FilterVersion {
        match self {
            FilterId::Legacy(_) => FilterVersion::Legacy,
            FilterId::V1(_, _) => FilterVersion::V1,
        }
    }
}

impl fmt::Display for FilterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilterId::Legacy(id) => write!(
                f,
                "FilterId::Legacy({})",
                str::from_utf8(id).unwrap_or("invalid filterid")
            ),
            FilterId::V1(_, id) => {
                let hash: String = id.iter().map(|b| format!("{:02x}", b)).collect();
                write!(f, "FilterId::V1({})", hash)
            }
        }
    }
}

#[allow(dead_code)]
impl FilterId {
    fn new(
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

#[allow(dead_code)]
pub struct Filter {
    filter_id: FilterId,
    filter_paths: Vec<RepoPathBuf>,
    commit_id: HgId,
}

#[allow(dead_code)]
impl Filter {
    // Default New constructor creates V1 Filters
    fn new(
        filter_paths: Vec<RepoPathBuf>,
        commit_id: HgId,
        filter_gen: &mut FilterGenerator,
    ) -> Result<Filter, anyhow::Error> {
        let filter_id = FilterId::new(
            FilterVersion::V1,
            &filter_paths,
            &commit_id,
            &filter_gen.hash_key,
        )?;
        let filter = Filter {
            filter_id,
            filter_paths,
            commit_id,
        };
        // TODO: Store the newly created filter
        Ok(filter)
    }

    fn new_legacy(filter_path: RepoPathBuf, commit_id: HgId) -> Result<Filter, anyhow::Error> {
        let filter_paths = vec![filter_path];
        let filter_id = FilterId::new(FilterVersion::Legacy, &filter_paths, &commit_id, &[0; 32])?;
        Ok(Filter {
            filter_id,
            commit_id,
            filter_paths,
        })
    }
}

pub(crate) struct FilterGenerator {
    dot_hg_path: PathBuf,
    filter_store: Store,
    hash_key: [u8; 32],
}

impl fmt::Display for FilterGenerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FilterGenerator {{ dot_hg_path: {:?}, hash_key: {} }}",
            self.dot_hg_path,
            to_hex(&self.hash_key)
        )
    }
}

#[allow(dead_code)]
impl FilterGenerator {
    pub fn new(
        dot_hg_path: PathBuf,
        filter_store_path: PathBuf,
        key: &[u8; Blake3::len()],
    ) -> anyhow::Result<Self> {
        // Filter content can be exceptionally long, so we store the actual filter content in an
        // indexedlog store and use a blake3 hash as the index to the filter contents. We avoid
        // long filter ids since EdenFS performance can degrade when ObjectID size grows too large.
        let config = BTreeMap::<&str, &str>::new();
        let filter_store = StoreOpenOptions::new(&config)
            .index("v1_filter_index", |_| {
                vec![IndexOutput::Reference(0..(Blake3::len() / 4) as u64)]
            })
            .permanent(&filter_store_path)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to open filter index store at {:?}: {:?}",
                    filter_store_path,
                    e
                )
            })?;

        let mut hash_key = [0u8; 32];
        hash_key.copy_from_slice(key);

        Ok(FilterGenerator {
            dot_hg_path,
            filter_store,
            hash_key,
        })
    }

    /// Check if a filter hash already exists in the store
    fn filter_exists(&self, filter_index: &[u8]) -> anyhow::Result<bool> {
        let store = self.filter_store.read();
        let lookup_iter = store
            .lookup(0, filter_index)
            .map_err(|e| anyhow::anyhow!("Failed to lookup filter hash in store: {:?}", e))?;

        Ok(!lookup_iter.is_empty()?)
    }

    // Takes a commit and returns the corresponding FilterID that should be passed to Eden.
    pub fn active_filter_id(&self, commit: HgId) -> Result<Option<String>, anyhow::Error> {
        // The filter file may be in 3 different states:
        //
        // 1) It may not exist, which indicates FilteredFS is not active
        // 2) It may contain nothing which indicates that FFS is in use, but no filter is active.
        // 3) It may contain the path to the active filter.
        //
        // We error out if the path exists but we can't read the file.
        let config_contents = std::fs::read_to_string(self.dot_hg_path.join("sparse"));
        let filter_path = match config_contents {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(anyhow::anyhow!(e)),
        };

        let filter_path = filter_path.trim();

        if filter_path.is_empty() {
            return Ok(None);
        }

        let filter_path = match filter_path.strip_prefix("%include ") {
            Some(p) => p,
            None => {
                warn!("Unexpected edensparse config format: {}", filter_path);
                return Ok(None);
            }
        };

        // Eden's ObjectIDs must be durable (once they exist, Eden must always be able to derive
        // the underlying object from them). FilteredObjectIDs contain FilterIDs, and therefore we
        // must be able to re-derive the filter contents from any FilterID so that we can properly
        // reconstruct the original filtered object at any future point in time. To do this, we
        // attach a commit ID to each FilterID which allows us to read Filter file contents from
        // the repo and reconstruct any filtered object.
        //
        // We construct a FilterID in the form {filter_file_path}:{hex_commit_hash}. We need to
        // parse this later to separate the path and commit hash, so this format assumes that
        // neither the filter file or the commit hash will have ":" in them. The second restriction
        // is guaranteed (hex), the first one will need to be enforced by us.
        Ok(Some(format!("{}:{}", filter_path, commit.to_hex())))
    }
}

#[cfg(test)]
mod tests {

    use mincode::deserialize;
    use mincode::serialize_into;
    use tempfile::TempDir;

    use super::*;

    // 32-byte test hash key for filter generator tests
    const TEST_HASH_KEY: &[u8; Blake3::len()] = b"01234567890123456789012345678901";

    // 40-character hex commit ID for tests
    const TEST_COMMIT_ID: &[u8] = b"1234567890123456789012345678901234567890";
    const TEST_COMMIT_ID_STR: &str = "1234567890123456789012345678901234567890";

    const DEFAULT_FILTER_PATH: &str = "path/to/filter.txt";

    fn create_test_filter_generator() -> (TempDir, FilterGenerator) {
        let temp_dir = TempDir::new().unwrap();
        let dot_hg_path = temp_dir.path().join(".hg");
        std::fs::create_dir_all(&dot_hg_path).unwrap();

        let filter_store_path = temp_dir.path().join("filter_store");

        let filter_gen =
            FilterGenerator::new(dot_hg_path, filter_store_path, TEST_HASH_KEY).unwrap();

        (temp_dir, filter_gen)
    }
    #[test]
    fn test_filter_version_display() {
        assert_eq!(FilterVersion::Legacy.to_string(), "Legacy");
        assert_eq!(FilterVersion::V1.to_string(), "V1");
    }

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

    #[test]
    fn test_filter_id_display() {
        // Test Legacy display
        let legacy_id_str = format!("{}:{}", DEFAULT_FILTER_PATH, TEST_COMMIT_ID_STR);
        let legacy_id = FilterId::Legacy(legacy_id_str.as_bytes().into());
        let display_str = legacy_id.to_string();
        assert_eq!(format!("FilterId::Legacy({})", legacy_id_str), display_str);
    }

    #[test]
    fn test_filter_new_legacy() {
        let filter_path = RepoPathBuf::from_utf8("test.txt".into()).unwrap();
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let filter = Filter::new_legacy(filter_path.clone(), commit_id.clone()).unwrap();

        assert!(matches!(filter.filter_id.version(), FilterVersion::Legacy));
        assert_eq!(filter.commit_id, commit_id);
        assert_eq!(filter.filter_paths, vec![filter_path.clone()]);
        assert_eq!(
            filter.filter_id.id().unwrap(),
            format!("{}:{}", filter_path, TEST_COMMIT_ID_STR).as_bytes()
        );
    }

    #[test]
    fn test_filter_generator_display() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator();
        let display_str = filter_gen.to_string();

        assert!(display_str.contains("FilterGenerator"));
        assert!(display_str.contains("dot_hg_path"));
        assert!(display_str.contains("hash_key"));
    }

    #[test]
    fn test_active_filter_id_no_config() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator();
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let result = filter_gen.active_filter_id(commit_id).unwrap();
        assert!(result.is_none());
    }
}
