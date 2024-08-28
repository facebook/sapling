/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str;
use std::time::SystemTime;

use anyhow::Result;
use base64::Engine;
use configset::Config;
use filetime::FileTime;
use log::error;
use log::info;
use serde_json::json;

use crate::error::*;
use crate::subscriber::Subscription;

static JOINED_DIR: &str = ".commitcloud";
static JOINED: &str = "joined";
static SEC_IN_WEEK: u64 = 604800;

/// Commit Cloud App ID
/// https://developers.facebook.com/apps/184975892288525/dashboard/
pub static COMMIT_CLOUD_APP_ID: u64 = 184975892288525u64;

/// Map from a subscription to list of repo roots
pub fn read_subscriptions(joined_pool_path: &Path) -> Result<HashMap<Subscription, Vec<PathBuf>>> {
    let mut joined_pool_path = joined_pool_path.to_path_buf();
    joined_pool_path.push(JOINED_DIR);
    joined_pool_path.push(JOINED);

    info!(
        "Reading subscription requests from '{}' folder...",
        joined_pool_path.display()
    );

    let paths = fs::read_dir(joined_pool_path);
    if let Err(e) = &paths {
        if e.kind() == io::ErrorKind::NotFound {
            info!("No active subscribers");
            return Ok(HashMap::new());
        }
        error!("{}", e);
    }

    let paths = paths?
        .filter(|result| result.is_ok())
        .map(|dir| dir.unwrap().path());

    let mut subscriptions: HashMap<Subscription, Vec<PathBuf>> = HashMap::new();

    for ref path in paths {
        let metadata = fs::metadata(path)?;
        let mtime = FileTime::from_last_modification_time(&metadata).unix_seconds() as u64;
        let timenow = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();
        if mtime + SEC_IN_WEEK < timenow {
            info!(
                "Removing old subscription file '{}' (mtime ts: {}). The client needs to resubscribe.",
                path.display(),
                mtime
            );
            let res = fs::remove_file(path);
            if res.is_ok() {
                continue;
            } else {
                info!(
                    "Failed to clean up old subscription file '{}'. The subscription remains active.",
                    path.display(),
                );
            }
        }

        if path.exists() {
            let mut config = configset::ConfigSet::new();
            let errors = config.load_path(path, &Default::default());
            if !errors.is_empty() {
                return Err(configset::Errors(errors).into());
            }

            let workspace = config.get_nonempty("commitcloud", "workspace");
            let repo_name = config.get_nonempty("commitcloud", "repo_name");
            let repo_root = config.get_nonempty("commitcloud", "repo_root");

            match (workspace, repo_name, repo_root) {
                (None, _, _) | (_, None, _) | (_, _, None) => {
                    info!(
                        "Skipping the file '{}' because format is invalid",
                        path.display()
                    );
                }
                (Some(workspace), Some(repo_name), Some(repo_root)) => {
                    let repo_root = PathBuf::from(repo_root.as_ref());
                    if !repo_root.exists() || !repo_root.is_dir() {
                        info!(
                            "Skipping the file '{}' because 'repo_root' '{}' \
                             is not an existing directory",
                            path.display(),
                            repo_root.display()
                        );
                        continue;
                    }
                    let subscription = Subscription {
                        repo_name: repo_name.to_string(),
                        workspace: workspace.to_string(),
                    };
                    if let Some(entry) = subscriptions.get_mut(&subscription) {
                        (*entry).push(repo_root);
                        continue;
                    }
                    subscriptions.insert(subscription, vec![repo_root]);
                }
            }
        }
    }

    info!(
        "Found {} active subscription{}",
        subscriptions.len(),
        if subscriptions.len() != 1 { "s" } else { "" }
    );

    for (key, value) in &subscriptions {
        info!(
            "Found {} subscription request{} for repo '{}' and workspace '{}'",
            value.len(),
            if value.len() != 1 { "s" } else { "" },
            key.repo_name,
            key.workspace
        );
    }
    Ok(subscriptions)
}

pub static TOKEN_FILENAME: &str = ".commitcloudrc";

#[derive(Clone, PartialEq, Debug)]
pub enum TokenType {
    OAuth,
    Cat,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TokenType::OAuth => write!(f, "oauth"),
            TokenType::Cat => write!(f, "cat"),
        }
    }
}

#[derive(Clone)]
pub struct Token {
    pub(crate) token: String,
    pub(crate) token_type: TokenType,
}

pub fn read_or_generate_access_token(user_token_path: &Option<PathBuf>) -> Result<Token> {
    // Try to read OAuth token from a file if exists
    // These Commit Cloud tokens are permanent unless revoked
    // Generated by: https://www.internalfb.com/intern/oauth/184975892288525
    // These tokens work for both Icebreaker and InternGraph
    let token = if let Some(user_token_path) = user_token_path {
        let mut user_token_path = user_token_path.clone();
        user_token_path.push(TOKEN_FILENAME);
        info!(
            "Token Lookup: reading commitcloud OAuth token from a file {}...",
            user_token_path.display()
        );

        if user_token_path.exists() {
            let mut config = configset::ConfigSet::new();
            let errors = config.load_path(user_token_path, &Default::default());
            if !errors.is_empty() {
                return Err(configset::Errors(errors).into());
            }

            config
                .get_nonempty("commitcloud", "user_token")
                .map(|t| t.to_string())
        } else {
            None
        }
    } else {
        None
    };
    if let Some(token) = token {
        return Ok(Token {
            token,
            token_type: TokenType::OAuth,
        });
    }

    // Try to issue a CAT token automatically.
    // These tokens generation work differently for Icebreaker and InternGraph
    let clicat_tool = if hostcaps::is_prod() {
        "clicat"
    } else {
        "corp_clicat"
    };
    info!(
        "Token Lookup: generating a CAT token via {} ...",
        clicat_tool
    );
    let token_timeout_seconds = 1200;
    let payload = base64::engine::general_purpose::STANDARD
        .encode(json!({"app":COMMIT_CLOUD_APP_ID}).to_string());
    let output = Command::new(clicat_tool)
        .args(vec![
            "create",
            "--verifier_type",
            "SERVICE_IDENTITY",
            "--verifier_id",
            "interngraph",
            "--token_timeout_seconds",
            &token_timeout_seconds.to_string(),
            "--payload",
            &payload,
        ])
        .output();

    match output {
        Err(e) => {
            if let io::ErrorKind::NotFound = e.kind() {
                info!("`{}` executable is not found", clicat_tool);
            } else {
                error!("`{}` failed: {}", clicat_tool, e)
            }
        }
        Ok(output) => {
            if !output.status.success() {
                error!(
                    "CAT token: failed to generate via {}, process exited with {}",
                    clicat_tool, output.status
                );
            } else {
                info!("CAT token has been generated");
                return Ok(Token {
                    token: str::from_utf8(&output.stdout)?.trim().to_string(),
                    token_type: TokenType::Cat,
                });
            }
        }
    }

    Err(ErrorKind::CommitCloudUnexpectedError("Token Lookup: token not found".into()).into())
}
