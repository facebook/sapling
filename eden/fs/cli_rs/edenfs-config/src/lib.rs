/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use stack_config::StackConfig;
use tracing::{event, Level};

#[derive(Deserialize, StackConfig, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Core {
    eden_directory: String,
}

#[derive(Deserialize, StackConfig, Debug)]
pub struct EdenFsConfig {
    #[stack(nested)]
    core: Core,
}

fn load_path(loader: &mut EdenFsConfigLoader, path: &Path) -> Result<()> {
    let content = String::from_utf8(std::fs::read(&path)?)?;
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

fn load_user(loader: &mut EdenFsConfigLoader, home_dir: &Path) -> Result<()> {
    let home_rc = home_dir.join(".edenrc");
    load_path(loader, &home_rc)
}

pub fn load_config(etc_eden_dir: &Path, home_dir: Option<&Path>) -> Result<EdenFsConfig> {
    let mut loader = EdenFsConfig::loader();

    if let Err(e) = load_system(&mut loader, &etc_eden_dir) {
        event!(
            Level::INFO,
            etc_eden_dir = ?etc_eden_dir,
            "Unable to load system configuration, skipped: {:?}",
            e
        );
    }

    if let Err(e) = load_system_rcs(&mut loader, &etc_eden_dir) {
        event!(
            Level::INFO,
            etc_eden_dir = ?etc_eden_dir,
            "Unable to load system RC configurations, skipped: {:?}",
            e
        );
    }

    if let Some(home) = home_dir {
        load_user(&mut loader, &home)?;
    } else {
        event!(
            Level::INFO,
            "Unable to find home dir. User configuration is not loaded."
        );
    }

    Ok(loader.build().map_err(|e| anyhow!("{}", e))?)
}
