/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
#[cfg(windows)]
use serde::Deserialize;

#[cfg(windows)]
#[derive(Deserialize)]
struct WindowsEdenConfigInner {
    root: PathBuf,
    client: PathBuf,
}

#[cfg(windows)]
#[derive(Deserialize)]
struct WindowsEdenConfig {
    #[serde(rename = "Config")]
    config: WindowsEdenConfigInner,
}

#[cfg(windows)]
pub fn get_client_dir(root: &Path) -> Result<PathBuf> {
    // On Windows, .eden/client is not a symlink. Parse .eden/config TOML instead.
    // Mirrors WindowsEdenConfig in eden/fs/cli_rs/edenfs-client/src/checkout.rs.
    // TODO: share this logic with edenfs-client/src/checkout.rs
    let eden_config_path = root.join(".eden").join("config");
    let content = fs_err::read_to_string(&eden_config_path)
        .with_context(|| format!("failed to read {}", eden_config_path.display()))?;
    let config: WindowsEdenConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", eden_config_path.display()))?;
    Ok(config.config.client)
}

#[cfg(not(windows))]
pub fn get_client_dir(root: &Path) -> Result<PathBuf> {
    let eden_client_link = root.join(".eden").join("client");
    fs_err::read_link(&eden_client_link)
        .with_context(|| format!("failed to read symlink {}", eden_client_link.display()))
}

pub fn build_eden_command(config: &dyn Config) -> Result<Command> {
    let eden_command = config.get_opt::<String>("edenfs", "command")?;
    let mut cmd = match eden_command {
        Some(cmd) => Command::new(cmd),
        None => anyhow::bail!("edenfs.command config is not set"),
    };

    // allow tests to specify different configuration directories from prod defaults
    if let Some(base_dir) = config.get_opt::<PathBuf>("edenfs", "basepath")? {
        cmd.args([
            "--config-dir".into(),
            base_dir.join("eden"),
            "--etc-eden-dir".into(),
            base_dir.join("etc_eden"),
            "--home-dir".into(),
            base_dir.join("home"),
        ]);
    }
    Ok(cmd)
}
