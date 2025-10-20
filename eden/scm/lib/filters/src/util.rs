/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::path::Path;

use configmodel::Config;
use configmodel::Text;
use types::RepoPathBuf;

pub fn filter_paths_from_config(config: &mut impl Config) -> Option<HashSet<Text>> {
    // Get unique set of filter paths
    let filter_paths = config
        .keys("clone")
        .iter()
        .filter_map(|k| {
            if k.starts_with("eden-sparse-filter") {
                tracing::debug!("found filter config key: {}", k);
                config.get("clone", k)
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
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
pub fn read_filter_config(dot_dir: &Path) -> anyhow::Result<Option<Vec<RepoPathBuf>>> {
    // The filter file may be in 3 different states:
    //
    // 1) It may not exist, which indicates FilteredFS is not active
    // 2) It may contain nothing which indicates that FFS is in use, but no filters are active.
    // 3) It may contain the paths to the active filters (one per line, each starting with "%include").
    //
    // We error out if the path exists, but we can't read the file.
    let config_contents = std::fs::read_to_string(dot_dir.join("sparse"));
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
            let trimmed = line.trim();
            if trimmed.starts_with("%include ") {
                if let Some(path) = line.strip_prefix("%include ") {
                    filter_paths.push(RepoPathBuf::from_string(path.trim().into())?);
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

        let result = filter_paths_from_config(&mut config);
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_filter_path() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "clone.eden-sparse-filter.1".to_string(),
            "path/to/filter".to_string(),
        );

        let result = filter_paths_from_config(&mut config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("path/to/filter"));
    }

    #[test]
    fn test_single_empty_filter() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert("clone.eden-sparse-filter".to_string(), "".to_string());

        let result = filter_paths_from_config(&mut config);
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

        let result = filter_paths_from_config(&mut config);
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

        let result = filter_paths_from_config(&mut config);
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
        assert_eq!(result[0].to_string(), "path/to/filter1.txt");
        assert_eq!(result[1].to_string(), "path/to/filter2.txt");
    }

    #[test]
    fn test_read_filter_config_with_comments() {
        let contents =
            "\t%include path/to/filter1.txt\n# This is a comment\n%include path/to/filter2.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].to_string(), "path/to/filter1.txt");
        assert_eq!(result[1].to_string(), "path/to/filter2.txt");

        let contents = "# A multi\n# Line comment\n%include path/to/filter1.txt\n\n\t# This is a comment\n%include path/to/filter2.txt\n";
        let (_tempdir, dot_dir) = setup_config_test(Some(contents));
        let result = read_filter_config(&dot_dir).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].to_string(), "path/to/filter1.txt");
        assert_eq!(result[1].to_string(), "path/to/filter2.txt");
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
}
