/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use configmodel::Config;
use configmodel::ConfigExt;
use types::RepoPathBuf;
use util::file::unlink_if_exists;
use util::fs_err;

use crate::util::backup_filter_config;
use crate::util::filter_config_path;
use crate::util::filter_paths_from_config;
use crate::util::read_filter_config;
use crate::util::write_filter_config;

#[derive(Debug, PartialEq)]
pub enum FilterSyncResult {
    /// No sparse file exists - not a FilteredFS repo
    NotFilteredFS,
    /// Filters already match configuration
    NoChangeNeeded,
    /// Filters were updated to match configuration
    Updated {
        previous: HashSet<RepoPathBuf>,
        current: HashSet<RepoPathBuf>,
    },
}

// Legacy marker file paths - kept for cleanup purposes
fn migration_marker_path(dot_dir: &Path) -> PathBuf {
    dot_dir.join("edensparse_migration")
}

fn migration_backup_path(dot_dir: &Path) -> PathBuf {
    dot_dir.join("edensparse_migration.bak")
}

/// Sync filters from config to the sparse file.
pub fn sync_filters(dot_dir: &Path, config: &dyn Config) -> anyhow::Result<FilterSyncResult> {
    let sparse_path = filter_config_path(dot_dir);
    if !sparse_path.exists() {
        return Ok(FilterSyncResult::NotFilteredFS);
    }

    let active_filters = read_filter_config(dot_dir)?.unwrap_or_default();
    let config_filters = filter_paths_from_config(config)
        .map(|set| {
            set.iter()
                .filter(|s| !s.as_ref().is_empty())
                .map(|s| RepoPathBuf::from_string(s.as_ref().into()))
                .collect::<Result<HashSet<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    if active_filters == config_filters {
        return Ok(FilterSyncResult::NoChangeNeeded);
    }

    backup_filter_config(dot_dir)?;
    let header = config
        .get_nonempty_opt("sparse", "filter-warning")
        .context("getting filter warning for filter sync")?;
    write_filter_config(&sparse_path, header, &config_filters)
        .context("writing synced filter config")?;

    Ok(FilterSyncResult::Updated {
        previous: active_filters,
        current: config_filters,
    })
}

/// Rollback filter sync by restoring from backup.
pub fn rollback_filter_sync(dot_dir: &Path) -> anyhow::Result<()> {
    let backup_path = dot_dir.join("sparse.bak");
    let sparse_path = filter_config_path(dot_dir);
    if backup_path.exists() {
        fs_err::rename(&backup_path, &sparse_path)
            .context("restoring filter config from backup")?;
    }
    Ok(())
}

/// Cleanup filter sync backup and legacy migration marker files.
pub fn cleanup_filter_sync_backup(dot_dir: &Path) -> anyhow::Result<()> {
    unlink_if_exists(dot_dir.join("sparse.bak"))?;
    // TODO: remove this once we no longer write the legacy marker
    unlink_if_exists(migration_marker_path(dot_dir))?;
    unlink_if_exists(migration_backup_path(dot_dir))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::util::tests::create_sparse_file;
    use crate::util::tests::create_test_dot_dir;

    fn make_config(filters: Vec<(&str, &str)>) -> BTreeMap<String, String> {
        filters
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_sync_filters_not_filteredfs() {
        // No sparse file exists → NotFilteredFS
        let (_tempdir, dot_dir) = create_test_dot_dir();
        let config: BTreeMap<String, String> = BTreeMap::new();
        let result = sync_filters(&dot_dir, &config).unwrap();
        assert_eq!(result, FilterSyncResult::NotFilteredFS);
    }

    #[test]
    fn test_sync_filters_no_change() {
        let (_tempdir, dot_dir) = create_test_dot_dir();
        create_sparse_file(&dot_dir, "%include path/to/filter").unwrap();
        let config = make_config(vec![("clone.eden-sparse-filter", "path/to/filter")]);
        let result = sync_filters(&dot_dir, &config).unwrap();
        assert_eq!(result, FilterSyncResult::NoChangeNeeded);
    }

    #[test]
    fn test_sync_filters_add_filters() {
        let (_tempdir, dot_dir) = create_test_dot_dir();
        create_sparse_file(&dot_dir, "").unwrap();
        let config = make_config(vec![("clone.eden-sparse-filter", "path/to/filter")]);
        let result = sync_filters(&dot_dir, &config).unwrap();
        match result {
            FilterSyncResult::Updated { previous, current } => {
                assert!(previous.is_empty());
                assert_eq!(current.len(), 1);
                assert!(
                    current.contains(&RepoPathBuf::from_string("path/to/filter".into()).unwrap())
                );
            }
            _ => panic!("Expected Updated result"),
        }
    }

    #[test]
    fn test_sync_filters_remove_filters() {
        let (_tempdir, dot_dir) = create_test_dot_dir();
        create_sparse_file(&dot_dir, "%include path/to/filter").unwrap();
        let config: BTreeMap<String, String> = BTreeMap::new();
        let result = sync_filters(&dot_dir, &config).unwrap();
        match result {
            FilterSyncResult::Updated { previous, current } => {
                assert_eq!(previous.len(), 1);
                assert!(
                    previous.contains(&RepoPathBuf::from_string("path/to/filter".into()).unwrap())
                );
                assert!(current.is_empty());
            }
            _ => panic!("Expected Updated result"),
        }
    }

    #[test]
    fn test_sync_filters_replace_filters() {
        let (_tempdir, dot_dir) = create_test_dot_dir();
        create_sparse_file(&dot_dir, "%include old/filter").unwrap();
        let config = make_config(vec![("clone.eden-sparse-filter", "new/filter")]);
        let result = sync_filters(&dot_dir, &config).unwrap();
        match result {
            FilterSyncResult::Updated { previous, current } => {
                assert_eq!(previous.len(), 1);
                assert!(previous.contains(&RepoPathBuf::from_string("old/filter".into()).unwrap()));
                assert_eq!(current.len(), 1);
                assert!(current.contains(&RepoPathBuf::from_string("new/filter".into()).unwrap()));
            }
            _ => panic!("Expected Updated result"),
        }
    }

    #[test]
    fn test_rollback_filter_sync() {
        let (_tempdir, dot_dir) = create_test_dot_dir();
        create_sparse_file(&dot_dir, "%include original/filter").unwrap();
        std::fs::copy(dot_dir.join("sparse"), dot_dir.join("sparse.bak")).unwrap();
        create_sparse_file(&dot_dir, "%include modified/filter").unwrap();
        rollback_filter_sync(&dot_dir).unwrap();
        let restored = std::fs::read_to_string(dot_dir.join("sparse")).unwrap();
        assert!(restored.contains("original/filter"));
        assert!(!restored.contains("modified/filter"));
    }
}
