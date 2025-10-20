/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use configmodel::Config;
use configmodel::Text;

pub fn filter_paths_from_config(config: &mut impl Config) -> Option<Vec<Text>> {
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
                .collect::<Vec<_>>(),
        )
    } else {
        Some(filter_paths.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

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
        assert_eq!(paths[0].as_ref(), "path/to/filter");
    }

    #[test]
    fn test_single_empty_filter() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert("clone.eden-sparse-filter".to_string(), "".to_string());

        let result = filter_paths_from_config(&mut config);
        let paths = result.unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_ref(), "");
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
        let mut path_strings: Vec<String> = paths.iter().map(|p| p.as_ref().to_string()).collect();
        path_strings.sort();
        assert_eq!(path_strings, vec!["path/to/filter1", "path/to/filter2"]);
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
        assert_eq!(paths[0].as_ref(), "path/to/filter");
    }
}
