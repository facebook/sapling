/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use tracing::warn;
use types::HgId;

#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(dead_code)]
enum FilterVersion {
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

pub(crate) struct FilterGenerator {
    dot_hg_path: PathBuf,
}

impl FilterGenerator {
    pub fn new(dot_hg_path: PathBuf) -> Self {
        FilterGenerator { dot_hg_path }
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

    use super::*;

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
}
