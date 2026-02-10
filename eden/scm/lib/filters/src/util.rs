/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use configmodel::Config;
use configmodel::Text;
use types::RepoPathBuf;
use util::file::atomic_write;
use util::fs_err;

pub fn filter_paths_from_config(config: &dyn Config) -> Option<HashSet<Text>> {
    let disabled_paths: HashSet<String> = config
        .keys("clone")
        .iter()
        .filter_map(|key| {
            if key.starts_with("disabled-eden-sparse-filter") {
                config.get("clone", key).map(|value| {
                    let path = value.to_string();
                    tracing::debug!("filter path disabled: {}", path);
                    path
                })
            } else {
                None
            }
        })
        .collect();

    let enabled_filters: Vec<Text> = config
        .keys("clone")
        .iter()
        .filter(|key| key.starts_with("eden-sparse-filter"))
        .filter_map(|key| config.get("clone", key))
        .collect();

    // Filter out enabled entries whose paths are in the disabled set
    let filter_paths: HashSet<Text> = enabled_filters
        .into_iter()
        .filter(|path| {
            let is_disabled = disabled_paths.contains(path.as_ref());
            if is_disabled {
                tracing::debug!(
                    "filter path {} is disabled by explicit disable config",
                    path.as_ref()
                );
            }
            !is_disabled
        })
        .collect();

    if filter_paths.is_empty() {
        None
    } else if filter_paths.len() > 1 {
        // If more than 1 filter path is supplied, remove "" as that represents the null filter and
        // cannot be combined with other filters
        Some(
            filter_paths
                .into_iter()
                .filter(|e| !e.as_ref().is_empty())
                .collect::<HashSet<_>>(),
        )
    } else {
        Some(filter_paths)
    }
}

// Parses the filter file and returns a list of active filter paths. Returns an error when the
// filter file is malformed or can't be read.
pub(crate) fn read_filter_config(dot_dir: &Path) -> anyhow::Result<Option<HashSet<RepoPathBuf>>> {
    // The filter file may be in 3 different states:
    //
    // 1) It may not exist, which indicates FilteredFS is not active
    // 2) It may contain nothing which indicates that FFS is in use, but no filters are active.
    // 3) It may contain the paths to the active filters (one per line, each starting with "%include").
    //
    // We error out if the path exists, but we can't read the file.
    let config_contents = std::fs::read_to_string(filter_config_path(dot_dir));
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
        let mut filter_paths = HashSet::new();
        for line in filter_contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("%include ") {
                if let Some(path) = line.strip_prefix("%include ") {
                    filter_paths.insert(RepoPathBuf::from_string(path.trim().into())?);
                }
            } else if trimmed.starts_with("#") {
                // Skip comments
                continue;
            } else if !trimmed.is_empty() {
                return Err(anyhow::anyhow!(
                    "Unexpected edensparse config format: {}",
                    line
                ));
            }
        }

        Ok(Some(filter_paths))
    }
}

// Writes a properly formatted filter config at the requested location. The file will be
// written regardless of whether FilteredFS is active.
pub(crate) fn write_filter_config(
    config_path: &Path,
    header: Option<String>,
    filter_paths: &HashSet<RepoPathBuf>,
) -> anyhow::Result<()> {
    let contents = if filter_paths.is_empty() {
        "".to_string()
    } else {
        let content = filter_paths
            .iter()
            .map(|p| format!("%include {}", p))
            .collect::<Vec<String>>()
            .join("\n");
        if let Some(header) = header {
            format!("{}\n\n{}", header, content)
        } else {
            content
        }
    };

    atomic_write(config_path, |f| write!(f, "{contents}"))
        .with_context(|| format!("writing filter config: {}", config_path.display()))?;
    Ok(())
}

pub(crate) fn filter_config_path(dot_dir: &Path) -> PathBuf {
    dot_dir.join("sparse")
}

pub(crate) fn backup_filter_config(dot_dir: &Path) -> anyhow::Result<()> {
    let sparse_path = filter_config_path(dot_dir);
    let backup_path = dot_dir.join("sparse.bak");
    if sparse_path.exists() {
        let contents = fs_err::read(&sparse_path)?;
        atomic_write(&backup_path, |f| f.write_all(&contents)).with_context(|| {
            format!(
                "backing up filter config from {} to {}",
                sparse_path.display(),
                backup_path.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    pub fn create_sparse_file(dot_dir: &Path, contents: &str) -> std::io::Result<()> {
        let sparse_path = dot_dir.join("sparse");
        let mut file = File::create(&sparse_path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }

    pub fn create_test_dot_dir() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let dot_dir = temp_dir.path().join(".hg");
        std::fs::create_dir_all(&dot_dir).unwrap();
        (temp_dir, dot_dir)
    }

    fn setup_config_test(sparse_file_content: Option<&str>) -> (TempDir, PathBuf) {
        let (temp_dir, dot_dir) = create_test_dot_dir();
        if let Some(sparse_file_content) = sparse_file_content {
            create_sparse_file(&dot_dir, sparse_file_content).unwrap();
        }
        (temp_dir, dot_dir)
    }

    #[test]
    fn test_no_filter_paths() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert("clone.other".to_string(), "value".to_string());

        let result = filter_paths_from_config(&config);
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_filter_path() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "clone.eden-sparse-filter.1".to_string(),
            "path/to/filter".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/filter"));
    }

    #[test]
    fn test_single_empty_filter() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert("clone.eden-sparse-filter".to_string(), "".to_string());

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains(""));
    }

    #[test]
    fn test_multiple_filter_paths() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "clone.eden-sparse-filter".to_string(),
            "path/to/filter1".to_string(),
        );
        config.insert(
            "clone.eden-sparse-filter.test".to_string(),
            "path/to/filter2".to_string(),
        );

        let result = filter_paths_from_config(&config);
        assert!(result.is_some());
        let paths = result.unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains("path/to/filter1"));
        assert!(paths.contains("path/to/filter2"));
    }

    #[test]
    fn test_duplicate_filter_paths() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "clone.eden-sparse-filter.1".to_string(),
            "path/to/filter".to_string(),
        );
        config.insert(
            "clone.eden-sparse-filter.2".to_string(),
            "path/to/filter".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/filter"));
    }

    #[test]
    fn test_read_filter_config_no_sparse_file() {
        // No sparse file exists
        let (_tempdir, dot_dir) = setup_config_test(None);
        let result = read_filter_config(&dot_dir).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_empty_file() {
        let (_tempdir, dot_dir) = setup_config_test(Some(""));
        let result = read_filter_config(&dot_dir).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_whitespace_only() {
        let (_tempdir, dot_dir) = setup_config_test(Some("   \n\t  \n"));
        let result = read_filter_config(&dot_dir).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_filter_config_valid_includes() {
        let contents = "\t%include path/to/filter1.txt\n%include path/to/filter2.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter1.txt".into()).unwrap()));
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter2.txt".into()).unwrap()));
    }

    #[test]
    fn test_read_filter_config_with_comments() {
        let contents =
            "\t%include path/to/filter1.txt\n# This is a comment\n%include path/to/filter2.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter1.txt".into()).unwrap()));
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter2.txt".into()).unwrap()));

        let contents = "# A multi\n# Line comment\n%include path/to/filter1.txt\n\n\t# This is a comment\n%include path/to/filter2.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter1.txt".into()).unwrap()));
        assert!(result.contains(&RepoPathBuf::from_string("path/to/filter2.txt".into()).unwrap()));
    }

    #[test]
    fn test_read_filter_config_invalid_format() {
        // Create sparse file with invalid line (not starting with %include)
        let contents = "invalid line\n%include valid.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir);

        assert!(result.is_err());
        match result {
            Ok(_tempdir) => panic!("result should be an error"),
            Err(e) => assert!(
                e.to_string()
                    .contains("Unexpected edensparse config format")
            ),
        };
    }

    #[test]
    fn test_multiple_filters_with_null_filter() {
        // Test that the null filter (empty string) is removed when other filters are present.
        // The null filter cannot be combined with other filters.
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        // Enable the null filter (no suffix, empty value)
        config.insert("clone.eden-sparse-filter".to_string(), "".to_string());
        // Enable two other filters
        config.insert(
            "clone.eden-sparse-filter.filter1".to_string(),
            "path/to/filter1".to_string(),
        );
        config.insert(
            "clone.eden-sparse-filter.filter2".to_string(),
            "path/to/filter2".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        // The null filter should be removed, leaving only the two real filters
        assert_eq!(paths.len(), 2);
        assert!(!paths.contains("")); // null filter removed
        assert!(paths.contains("path/to/filter1"));
        assert!(paths.contains("path/to/filter2"));
    }

    #[test]
    fn test_disable_overrides_enable() {
        // When a filter is both enabled and disabled via the dedicated section,
        // the disable entry should take precedence.
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        // Enable filter with alias "tent" pointing to "tools/tent/path1"
        config.insert(
            "clone.eden-sparse-filter.tent".to_string(),
            "tools/tent/path1".to_string(),
        );
        // Disable the same path via dedicated section
        config.insert(
            "clone.disabled-eden-sparse-filter.tent".to_string(),
            "tools/tent/path1".to_string(),
        );

        let result = filter_paths_from_config(&config);
        // The filter should be disabled, so result should be None
        assert!(result.is_none());
    }

    #[test]
    fn test_multiple_filters_with_one_disabled() {
        // When multiple filters exist and one is disabled, only the non-disabled ones
        // should be returned.
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        // Enable two filters
        config.insert(
            "clone.eden-sparse-filter.filter1".to_string(),
            "path/to/filter1".to_string(),
        );
        config.insert(
            "clone.eden-sparse-filter.filter2".to_string(),
            "path/to/filter2".to_string(),
        );
        // Disable filter1 via dedicated section
        config.insert(
            "clone.disabled-eden-sparse-filter.filter1".to_string(),
            "path/to/filter1".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        // Only filter2 should remain
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/filter2"));
        assert!(!paths.contains("path/to/filter1"));
    }

    #[test]
    fn test_disable_does_not_affect_other_paths() {
        // Disabling one path should not affect filters with different paths.
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        // Enable a filter
        config.insert(
            "clone.eden-sparse-filter.myfilter".to_string(),
            "path/to/myfilter".to_string(),
        );
        // Disable a completely different path via dedicated section
        config.insert(
            "clone.disabled-eden-sparse-filter.other".to_string(),
            "other/path/here".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        // myfilter should still be enabled
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/myfilter"));
    }

    #[test]
    fn test_eden_sparse_filter_with_path_value() {
        // Test that "clone.eden-sparse-filter=path" (no suffix, non-empty path value)
        // is correctly parsed and returns the path.
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "clone.eden-sparse-filter".to_string(),
            "path/to/filter".to_string(),
        );

        let result = filter_paths_from_config(&config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/filter"));
    }
}
