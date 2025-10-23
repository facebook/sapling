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
use configmodel::Text;
use types::RepoPathBuf;
use util::file::unlink_if_exists;

use crate::util::filter_config_path;
use crate::util::filter_paths_from_config;
use crate::util::read_filter_config;
use crate::util::write_filter_config;

fn migration_marker_path(dot_dir: &Path) -> PathBuf {
    dot_dir.join("edensparse_migration")
}

fn migration_backup_path(dot_dir: &Path) -> PathBuf {
    dot_dir.join("edensparse_migration.bak")
}

fn is_migration_required(dot_dir: &Path) -> bool {
    match migration_marker_path(dot_dir).try_exists() {
        Ok(res) => res,
        Err(e) => {
            tracing::error!(
                "checking migration marker, assuming no migration is needed: {:?}",
                e
            );
            false
        }
    }
}

fn prepare_migration_context(
    dot_dir: &Path,
    active_filters: &HashSet<RepoPathBuf>,
    config_filters: &HashSet<Text>,
    config: &dyn Config,
) -> anyhow::Result<()> {
    let config_filter_paths = config_filters
        .iter()
        .map(|c| RepoPathBuf::from_string(c.as_ref().into()))
        .collect::<Result<HashSet<_>, _>>()?;

    if config_filter_paths.is_empty() || config_filter_paths.difference(active_filters).count() == 0
    {
        // Migration was requested, but no filters need to be applied to the repo. Cleanup the
        // migration file so that we don't reattempt migration in the future.
        unlink_if_exists(migration_marker_path(dot_dir)).ok();
        return Ok(());
    }

    // Filter(s) must be activated. Backup existing filters and start the migration
    let migration_filters = config_filter_paths
        .union(active_filters)
        .cloned()
        .collect::<HashSet<RepoPathBuf>>();

    let header = config
        .get_nonempty_opt("sparse", "filter-warning")
        .context("getting filter warning for filter migration")?;

    write_filter_config(
        &migration_backup_path(dot_dir),
        header.clone(),
        active_filters,
    )
    .context("preparing backup filter config")?;

    if let Err(e) = write_filter_config(&filter_config_path(dot_dir), header, &migration_filters) {
        // Cleanup and return error
        return match unlink_if_exists(migration_backup_path(dot_dir)) {
            Err(cleanup_err) => Err(e.context(format!(
                "error while cleaning up failed migration: {}",
                cleanup_err
            ))),
            Ok(_) => Err(e),
        };
    }
    Ok(())
}

#[allow(dead_code)]
pub fn prepare_migration(dot_dir: &Path, config: &dyn Config) -> anyhow::Result<()> {
    if is_migration_required(dot_dir) {
        let active_filters = read_filter_config(dot_dir)?.unwrap_or_default();
        let config_filters = filter_paths_from_config(config).unwrap_or_default();
        prepare_migration_context(dot_dir, &active_filters, &config_filters, config)
            .context("initiating filter migration")?;
    }
    Ok(())
}

#[allow(dead_code)]
pub fn cleanup_migration(dot_dir: &Path, success: bool) -> anyhow::Result<()> {
    if success {
        // Note: failure doesn't matter; next migration attempt will notice there's no work to be
        // done, short circuit, an reattempt cleanup
        unlink_if_exists(migration_marker_path(dot_dir)).ok();
    } else {
        std::fs::rename(migration_backup_path(dot_dir), filter_config_path(dot_dir))
            .context("restoring filter migration backup state")?;
    }

    // Unconditionally cleanup the migration backup file
    unlink_if_exists(migration_backup_path(dot_dir))?;
    Ok(())
}
