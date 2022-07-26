/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use stack_config::StackConfig;
use tracing::event;
use tracing::trace;
use tracing::Level;

use edenfs_error::EdenFsError;

#[derive(Serialize, Deserialize, StackConfig, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Core {
    #[stack(default)]
    eden_directory: Option<String>,
}

#[derive(Serialize, Deserialize, StackConfig, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct PrefetchProfiles {
    #[stack(default)]
    pub prefetching_enabled: bool,

    #[stack(default)]
    pub predictive_prefetching_enabled: bool,

    #[stack(merge = "merge_table")]
    #[serde(flatten)]
    pub other: toml::value::Table,
}

#[derive(Serialize, Deserialize, StackConfig, Debug)]
pub struct EdenFsConfig {
    #[stack(nested)]
    #[serde(skip_serializing_if = "skip_core_serialization")]
    pub core: Core,

    #[serde(rename = "prefetch-profiles")]
    pub prefetch_profiles: PrefetchProfiles,

    #[stack(merge = "merge_table")]
    #[serde(flatten)]
    /// A catch-all field for unused configuration fields. If you need
    /// to use any configurations, define them above instead of reading from
    /// this field.
    pub other: toml::value::Table,
}

fn skip_core_serialization(core: &Core) -> bool {
    core.eden_directory.is_none()
}

fn merge_table(lhs: &mut toml::value::Table, rhs: toml::value::Table) {
    for (key, value) in rhs.into_iter() {
        if let Some(lhs_value) = lhs.get_mut(&key) {
            // Key exists
            if let (Some(lhs_table), true) = (lhs_value.as_table_mut(), value.is_table()) {
                // Both value are table, we merge them
                // SAFETY: unwrap here is guaranteed by `value.is_table()`. This
                // is awkward because try_into will consume the value, making
                // the else-clause not able to use it later.
                merge_table(lhs_table, value.try_into::<toml::value::Table>().unwrap());
            } else {
                // Otherwise, either the values are not table type, or they have
                // different types. In both case we prefer rhs value.
                *lhs_value = value;
            }
        } else {
            // Key does not exist in lhs
            lhs.insert(key, value);
        }
    }
}

fn load_path(loader: &mut EdenFsConfigLoader, path: &Path) -> Result<()> {
    let content = String::from_utf8(std::fs::read(&path)?)?;
    trace!(?content, ?path, "Loading config");
    loader.load(toml::from_str(&content)?);
    Ok(())
}

fn load_system(loader: &mut EdenFsConfigLoader, etc_dir: &Path) -> Result<()> {
    load_path(loader, &etc_dir.join("edenfs.rc"))
}

fn load_system_rcs(loader: &mut EdenFsConfigLoader, etc_dir: &Path) -> Result<()> {
    let rcs_dir = etc_dir.join("config.d");
    let entries = std::fs::read_dir(&rcs_dir)
        .with_context(|| format!("Unable to read configuration from {:?}", rcs_dir))?;

    for rc in entries {
        let rc = match rc {
            Ok(rc) => rc,
            Err(e) => {
                event!(
                    Level::INFO,
                    "Unable to read configuration, skipped: {:?}",
                    e
                );
                continue;
            }
        };
        let name = rc.file_name();
        let name = if let Some(name) = name.to_str() {
            name
        } else {
            continue;
        };

        if name.starts_with('.') || !name.ends_with(".toml") {
            continue;
        }

        if let Err(e) = load_path(loader, &rc.path()) {
            event!(
                Level::DEBUG,
                "Not able to load '{}': {:?}",
                rc.path().display(),
                e
            );
        }
    }

    Ok(())
}

pub fn load_user(loader: &mut EdenFsConfigLoader, home_dir: &Path) -> Result<()> {
    let home_rc = home_dir.join(".edenrc");
    load_path(loader, &home_rc)
}

pub fn load_config(
    etc_eden_dir: &Path,
    home_dir: Option<&Path>,
) -> Result<EdenFsConfig, EdenFsError> {
    let mut loader = EdenFsConfig::loader();

    if let Err(e) = load_system(&mut loader, &etc_eden_dir) {
        event!(
            Level::INFO,
            etc_eden_dir = ?etc_eden_dir,
            "Unable to load system configuration, skipped: {:?}",
            e
        );
    } else {
        event!(Level::DEBUG, "System configuration loaded");
    }

    if let Err(e) = load_system_rcs(&mut loader, &etc_eden_dir) {
        event!(
            Level::INFO,
            etc_eden_dir = ?etc_eden_dir,
            "Unable to load system RC configurations, skipped: {:?}",
            e
        );
    } else {
        event!(Level::DEBUG, "System RC configurations loaded");
    }

    if let Some(home) = home_dir {
        if let Err(e) = load_user(&mut loader, &home) {
            event!(Level::INFO, home = ?home, "Unable to load user configuration, skipped: {:?}", e);
        } else {
            event!(Level::DEBUG, "User configuration loaded");
        }
    } else {
        event!(
            Level::INFO,
            "Unable to find home dir. User configuration is not loaded."
        );
    }

    Ok(loader.build().map_err(EdenFsError::ConfigurationError)?)
}
