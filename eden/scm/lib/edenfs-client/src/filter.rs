/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use blake3::Hasher as Blake3Hasher;
use configmodel::Config;
use configmodel::ConfigExt;
use derivative::Derivative;
use indexedlog::log::IndexOutput;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Filter {
    pub filter_id: FilterId,
    pub filter_paths: Vec<RepoPathBuf>,
    pub commit_id: HgId,
}

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
        // Enforce that filters are persisted in storage. No-op if filter is already stored.
        filter_gen.store_filter(&filter)?;
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

#[derive(Derivative)]
#[derivative(Debug)]
pub struct FilterGenerator {
    dot_dir: PathBuf,
    #[derivative(Debug = "ignore")]
    filter_store: Option<Store>,
    hash_key: [u8; 32],
    // Allows slow rollout of new filter versions
    default_filter_version: FilterVersion,
}

impl FilterGenerator {
    /// Creates a filter generator that looks for the .{hg,sl}/sparse file in the repo dir, but
    /// places the filter storage indexedlog in the shared dot dir. This allows filter storage to
    /// be shared amongst EdenFS repos of the same type, while allowing spareness to be determined
    /// per repository.
    pub fn from_dot_dirs(
        dot_dir: &Path,
        shared_dot_dir: &Path,
        config: &dyn Config,
    ) -> anyhow::Result<Self> {
        let version_config: Option<String> = config.get_opt("experimental", "filter-version")?;
        let use_filter_storage = config.get_or("experimental", "use-filter-storage", || true)?;
        let filter_version = version_config.map_or(FilterVersion::Legacy, |v| {
            FilterVersion::from_str(&v).unwrap_or_else(|e| {
                tracing::warn!("provided filter version is invalid: {:?}", e);
                FilterVersion::Legacy
            })
        });
        FilterGenerator::new(
            dot_dir.to_path_buf(),
            filter_version,
            use_filter_storage,
            Some(shared_dot_dir.join("filters")),
            None,
        )
    }

    pub fn new(
        dot_dir: PathBuf,
        default_filter_version: FilterVersion,
        use_filter_store: bool,
        filter_store_path: Option<PathBuf>,
        key: Option<&[u8; Blake3::len()]>,
    ) -> anyhow::Result<Self> {
        // Filter content can be exceptionally long, so we store the actual filter content in an
        // indexedlog store and use a blake3 hash as the index to the filter contents. We avoid
        // long filter ids since EdenFS performance can degrade when ObjectID size grows too large.
        let (filter_store, default_version) = if use_filter_store {
            let filter_store_path = filter_store_path.unwrap_or_else(|| dot_dir.join("filters"));
            let config = BTreeMap::<&str, &str>::new();
            (
                Some(
                    StoreOpenOptions::new(&config)
                        .index("v1_filter_index", |_| {
                            vec![IndexOutput::Reference(0..(Blake3::len() / 4) as u64)]
                        })
                        .permanent(&filter_store_path)
                        .with_context(|| {
                            anyhow::anyhow!(
                                "Failed to open filter index store at {:?}",
                                filter_store_path
                            )
                        })?,
                ),
                default_filter_version,
            )
        } else {
            // If filter storage is disabled, we shouldn't try to construct new V1 filter IDs
            (None, FilterVersion::Legacy)
        };

        #[cfg(fbcode_build)]
        let key = key.unwrap_or(blake3_constants::BLAKE3_HASH_KEY);
        #[cfg(not(fbcode_build))]
        let key = key.unwrap_or(b"20220728-2357111317192329313741#");

        let mut hash_key = [0u8; 32];
        hash_key.copy_from_slice(key);

        Ok(FilterGenerator {
            dot_dir,
            filter_store,
            hash_key,
            default_filter_version: default_version,
        })
    }

    /// Check if a filter hash already exists in the store
    fn filter_exists(&self, filter_index: &[u8]) -> anyhow::Result<bool> {
        if let Some(filter_store) = &self.filter_store {
            let store = filter_store.read();
            let lookup_iter = store
                .lookup(0, filter_index)
                .with_context(|| anyhow::anyhow!("Failed to lookup filter hash in store"))?;

            Ok(!lookup_iter.is_empty()?)
        } else {
            Err(anyhow::anyhow!(
                "Tried to check for existing filter {:?}, but filter storage is disabled",
                filter_index
            ))
        }
    }

    fn store_filter(&mut self, filter: &Filter) -> anyhow::Result<()> {
        if let Some(filter_store) = &self.filter_store {
            if self.filter_exists(filter.filter_id.index())? {
                Ok(())
            } else {
                // Store the entry
                filter_store
                    .append_direct(|buffer| {
                        buffer.extend_from_slice(filter.filter_id.index());
                        mincode::serialize_into(buffer, &filter)?;
                        Ok(())
                    })
                    .with_context(|| anyhow::anyhow!("Failed to add filter to store"))?;

                // Flush to ensure it's written to disk
                filter_store
                    .flush()
                    .with_context(|| anyhow::anyhow!("Failed to flush filter store to disk"))?;

                Ok(())
            }
        } else {
            Err(anyhow::anyhow!(
                "Tried to store V1 filter {:?}, but filter storage is disabled",
                filter.filter_id,
            ))
        }
    }

    /// Get Filter content from a filter str
    #[allow(dead_code)]
    pub fn get_filter_from_bytes<T: AsRef<[u8]>>(&self, filter_id: T) -> anyhow::Result<Filter> {
        let parsed_id = FilterId::from_bytes(filter_id.as_ref())?;
        match parsed_id {
            FilterId::Legacy(id) => unsafe {
                // from_bytes guarantees that the bytes are valid utf8 and contains just 1 ":"
                let s = str::from_utf8_unchecked(&id);
                let mut it = s.split(":");
                let path_str = it.next().expect("Legacy filter id has 2 components");
                let filter_path = RepoPathBuf::from_string(path_str.into())?;
                let commit_str = it.next().expect("Legacy filter id has 2 components");
                let commit_id = HgId::from_str(commit_str)?;
                Filter::new_legacy(filter_path, commit_id)
            },
            FilterId::V1 { .. } => self.get_filter_from_storage(&parsed_id),
        }
    }

    /// Get stored Filter content using a FilterID
    fn get_filter_from_storage(&self, id: &FilterId) -> anyhow::Result<Filter> {
        if let Some(filter_store) = &self.filter_store {
            let store = filter_store.read();
            let mut lookup_iter = store.lookup(0, id.index()).with_context(|| {
                anyhow::anyhow!("Failed to find filter with index {:?}", id.index())
            })?;

            match lookup_iter.next() {
                Some(Ok(entry)) => mincode::deserialize(&entry[id.index().iter().count()..])
                    .with_context(|| {
                        anyhow::anyhow!("Invalid stored filter with index ({:?})", id.index())
                    }),
                Some(Err(e)) => Err(e),
                None => Err(anyhow::anyhow!(
                    "Failed to find a stored Filter for ID: {:?}",
                    id
                )),
            }
        } else {
            Err(anyhow::anyhow!(
                "Tried to fetch V1 filter {:?}, but filter storage is disabled",
                id,
            ))
        }
    }

    // Parses the filter file and returns a list of active filter paths. Returns an error when the
    // filter file is malformed or can't be read.
    fn read_filter_config(&self) -> anyhow::Result<Option<Vec<RepoPathBuf>>> {
        // The filter file may be in 3 different states:
        //
        // 1) It may not exist, which indicates FilteredFS is not active
        // 2) It may contain nothing which indicates that FFS is in use, but no filters are active.
        // 3) It may contain the paths to the active filters (one per line, each starting with "%include").
        //
        // We error out if the path exists, but we can't read the file.
        let config_contents = std::fs::read_to_string(self.dot_dir.join("sparse"));
        let filter_contents = match config_contents {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(anyhow::anyhow!(e)),
        };

        let filter_contents = filter_contents.trim();

        if filter_contents.is_empty() {
            Ok(None)
        } else {
            // Parse each line that starts with "%include" to extract filter paths
            let mut filter_paths = Vec::new();
            for line in filter_contents.lines() {
                if line.starts_with("%include ") {
                    if let Some(path) = line.strip_prefix("%include ") {
                        filter_paths.push(RepoPathBuf::from_string(path.trim().into())?);
                    }
                } else if !line.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Unexpected edensparse config format: {}",
                        line
                    ));
                }
            }

            Ok(Some(filter_paths))
        }
    }

    // Takes a commit and returns the corresponding FilterID that should be passed to Eden.
    pub fn active_filter_id(
        &mut self,
        commit_id: &HgId,
    ) -> Result<Option<FilterId>, anyhow::Error> {
        if let Some(filter_paths) = self.read_filter_config()? {
            let filter = match self.default_filter_version {
                FilterVersion::Legacy if filter_paths.len() == 1 => {
                    // Legacy filters only support a single filter path
                    Filter::new_legacy(filter_paths[0].clone(), commit_id.clone())?
                }
                FilterVersion::V1 => Filter::new(filter_paths, commit_id.clone(), self)?,
                FilterVersion::Legacy => {
                    return Err(anyhow::anyhow!(
                        "V1 filters are disabled, but multiple filter paths are specified"
                    ));
                }
            };
            Ok(Some(filter.filter_id))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

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

    fn create_test_filter_generator(
        filter_version: FilterVersion,
        use_storage: Option<bool>,
    ) -> (TempDir, FilterGenerator) {
        let temp_dir = TempDir::new().unwrap();
        let dot_dir = temp_dir.path().join(".hg");
        std::fs::create_dir_all(&dot_dir).unwrap();

        let filter_store_path = temp_dir.path().join("filter_store");

        let filter_gen = FilterGenerator::new(
            dot_dir.clone(),
            filter_version,
            use_storage.unwrap_or(true),
            Some(filter_store_path),
            Some(TEST_HASH_KEY),
        )
        .unwrap();

        (temp_dir, filter_gen)
    }

    fn create_sparse_file(dot_dir: &Path, contents: &str) -> std::io::Result<()> {
        let sparse_path = dot_dir.join("sparse");
        let mut file = File::create(&sparse_path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
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
    fn test_read_filter_config_no_sparse_file() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        // No sparse file exists
        let result = filter_gen.read_filter_config().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_empty_file() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        create_sparse_file(&filter_gen.dot_dir, "").unwrap();
        let result = filter_gen.read_filter_config().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_whitespace_only() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        create_sparse_file(&filter_gen.dot_dir, "   \n\t  \n").unwrap();
        let result = filter_gen.read_filter_config().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_valid_includes() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        let contents = "%include path/to/filter1.txt\n%include path/to/filter2.txt\n";
        create_sparse_file(&filter_gen.dot_dir, contents).unwrap();
        let result = filter_gen.read_filter_config().unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].to_string(), "path/to/filter1.txt");
        assert_eq!(result[1].to_string(), "path/to/filter2.txt");
    }

    #[test]
    fn test_read_filter_config_invalid_format() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);

        // Create sparse file with invalid line (not starting with %include)
        let contents = "invalid line\n%include valid.txt\n";
        create_sparse_file(&filter_gen.dot_dir, contents).unwrap();

        let result = filter_gen.read_filter_config();
        assert!(result.is_err());
        match result {
            Ok(_) => panic!("result should be an error"),
            Err(e) => assert!(
                e.to_string()
                    .contains("Unexpected edensparse config format")
            ),
        };
    }

    #[test]
    fn test_active_filter_id_no_config() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let result = filter_gen.active_filter_id(&commit_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_active_filter_id_with_filters() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::V1, None);
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        // Create sparse file with filters
        let contents = format!("%include {}\n", DEFAULT_FILTER_PATH);
        create_sparse_file(&filter_gen.dot_dir, &contents).unwrap();

        let result = filter_gen.active_filter_id(&commit_id).unwrap();
        assert!(result.is_some());

        // Verify the filter was stored correctly
        if let Some(active_fid) = result {
            let id = active_fid.id().unwrap();
            let stored_filter = filter_gen.get_filter_from_bytes(id.clone()).unwrap();
            assert_eq!(stored_filter.filter_id.id().unwrap(), id);
            assert_eq!(stored_filter.commit_id, commit_id);
        } else {
            panic!("Expected V1 FilterId");
        }
    }

    #[test]
    fn test_get_filter_legacy() {
        let (_tmp_dir, filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);

        let legacy_id_str = &format!("{}:{}", DEFAULT_FILTER_PATH, TEST_COMMIT_ID_STR);
        let filter = filter_gen
            .get_filter_from_bytes(legacy_id_str.as_bytes())
            .unwrap();

        assert!(matches!(filter.filter_id.version(), FilterVersion::Legacy));
        assert_eq!(filter.filter_paths[0].to_string(), DEFAULT_FILTER_PATH);
        assert_eq!(filter.commit_id, HgId::from_hex(TEST_COMMIT_ID).unwrap());
        assert_eq!(
            &filter.filter_id.id().expect("to be valid utf8"),
            legacy_id_str.as_bytes()
        );
    }

    #[test]
    fn test_filter_roundtrip_v1() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::V1, None);

        // Create a V1 filter
        let filter_paths = vec![RepoPathBuf::from_string("test/filter.txt".to_string()).unwrap()];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let filter = Filter::new(filter_paths, commit_id, &mut filter_gen).unwrap();

        // Serialize and deserialize
        let ser = mincode::serialize(&filter).unwrap();
        let deserialized: Filter = deserialize(&ser).unwrap();

        // Verify the deserialized filter matches
        assert!(matches!(
            deserialized.filter_id.version(),
            FilterVersion::V1
        ));
        assert_eq!(deserialized.commit_id, filter.commit_id);
        assert_eq!(deserialized.filter_paths, filter.filter_paths);
        assert_eq!(
            deserialized.filter_id.id().unwrap(),
            filter.filter_id.id().unwrap()
        );
        assert_eq!(deserialized.filter_id.index(), filter.filter_id.index());
    }

    #[test]
    fn test_new_filter_multiple_inserts() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::V1, None);

        let filter_paths = vec![RepoPathBuf::from_string("test/filter.txt".to_string()).unwrap()];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();
        let filter = Filter::new(filter_paths.clone(), commit_id.clone(), &mut filter_gen).unwrap();

        // Create the filter again, which causes it to be "stored" again (should be no-op)
        let _ = Filter::new(filter_paths.clone(), commit_id.clone(), &mut filter_gen).unwrap();

        // Serialize and deserialize
        let ser = mincode::serialize(&filter).unwrap();
        let deserialized: Filter = mincode::deserialize(&ser).unwrap();

        // Verify the deserialized filter matches
        assert!(matches!(
            deserialized.filter_id.version(),
            FilterVersion::V1
        ));
        assert_eq!(deserialized.commit_id, filter.commit_id);
        assert_eq!(deserialized.filter_paths, filter.filter_paths);
        assert_eq!(
            deserialized.filter_id.id().unwrap(),
            filter.filter_id.id().unwrap()
        );
    }

    #[test]
    fn test_filter_with_multiple_paths() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::V1, None);

        let filter_paths = vec![
            RepoPathBuf::from_utf8("path1.txt".into()).unwrap(),
            RepoPathBuf::from_utf8("path2.txt".into()).unwrap(),
            RepoPathBuf::from_utf8("path3.txt".into()).unwrap(),
        ];
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        let filter = Filter::new(filter_paths.clone(), commit_id.clone(), &mut filter_gen).unwrap();
        let stored_filter = filter_gen
            .get_filter_from_bytes(filter.filter_id.id().unwrap())
            .expect("to be stored");

        assert_eq!(filter.filter_paths, filter_paths);
        assert_eq!(filter.commit_id, commit_id);
        assert!(matches!(filter.filter_id.version(), FilterVersion::V1));
        assert_eq!(
            stored_filter.filter_id.id().unwrap(),
            filter.filter_id.id().unwrap()
        );
    }

    #[test]
    fn test_active_filter_id_legacy_single_path() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::Legacy, None);
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();
        let contents = "%include path/to/filter.txt\n";
        create_sparse_file(&filter_gen.dot_dir, contents).unwrap();

        let result = filter_gen.active_filter_id(&commit_id).unwrap().unwrap();

        // With Legacy version and single path, should create a Legacy FilterId
        if let FilterId::Legacy(id) = result {
            assert!(str::from_utf8(&id).unwrap().contains("path/to/filter.txt"));
            assert!(str::from_utf8(&id).unwrap().contains(&commit_id.to_hex()));
        } else {
            panic!("Expected Legacy FilterId for single path with Legacy version");
        }
    }

    #[test]
    fn test_active_filter_id_legacy_multiple_paths_with_no_storage() {
        let (_tmp_dir, mut filter_gen) =
            create_test_filter_generator(FilterVersion::V1, Some(false));
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();
        let contents = "%include path/to/filter1.txt\n%include path/to/filter2.txt\n";
        create_sparse_file(&filter_gen.dot_dir, contents).unwrap();

        let result = filter_gen.active_filter_id(&commit_id);

        // With Legacy version but multiple paths, active_filter_id will fail
        match result {
            Ok(_) => {
                panic!("Legacy filters should not support multiple paths");
            }
            Err(e) => {
                assert!(e.to_string().contains("V1 filters are disabled"));
            }
        }
    }

    #[test]
    fn test_active_filter_id_v1_single_path() {
        let (_tmp_dir, mut filter_gen) = create_test_filter_generator(FilterVersion::V1, None);
        let commit_id = HgId::from_hex(TEST_COMMIT_ID).unwrap();

        // Create sparse file with a single filter path
        let contents = "%include path/to/filter.txt\n";
        create_sparse_file(&filter_gen.dot_dir, contents).unwrap();

        let result = filter_gen.active_filter_id(&commit_id).unwrap().unwrap();

        // With V1 version, should always create V1 FilterId regardless of path count
        match result {
            FilterId::V1(ver, _) => {
                assert_eq!(ver, FilterVersion::V1);
            }
            FilterId::Legacy { .. } => {
                panic!("Expected V1 FilterId when using V1 version");
            }
        }
    }
}
