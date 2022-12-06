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

use anyhow::bail;
use anyhow::Result;
use filetime::FileTime;
use ini::Ini;
use log::error;
use log::info;

use crate::error::*;
use crate::subscriber::Subscription;

static JOINED_DIR: &str = ".commitcloud";
static JOINED: &str = "joined";
static SEC_IN_WEEK: u64 = 604800;

/// Map from a subscription to list of repo roots
pub fn read_subscriptions(
    joined_pool_path: &PathBuf,
) -> Result<HashMap<Subscription, Vec<PathBuf>>> {
    let mut joined_pool_path = joined_pool_path.clone();
    joined_pool_path.push(JOINED_DIR);
    joined_pool_path.push(JOINED);

    info!(
        "Reading subscription requests from '{}' folder...",
        joined_pool_path.display()
    );

    let paths = fs::read_dir(joined_pool_path);
    if let &Err(ref e) = &paths {
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
        if let Ok(ref mut file) = fs::OpenOptions::new().read(true).open(path) {
            let ini = Ini::read_from(&mut io::BufReader::new(file))?;
            let section = ini.section(Some("commitcloud"));
            if let Some(section) = section {
                // strip whitespaces around the fields
                let workspace = section.get("workspace").map(|workspace| workspace.trim());
                let repo_name = section.get("repo_name").map(|repo_name| repo_name.trim());
                let repo_root = section
                    .get("repo_root")
                    .map(|repo_root| PathBuf::from(repo_root.trim()));

                if workspace.is_none() || repo_name.is_none() || repo_root.is_none() {
                    info!(
                        "Skipping the file '{}' because format is invalid",
                        path.display()
                    );
                } else {
                    let workspace = workspace.unwrap();
                    let repo_name = repo_name.unwrap();
                    let repo_root = repo_root.unwrap();

                    if !Path::new(&repo_root).exists() || !Path::new(&repo_root).is_dir() {
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
                    {
                        if let Some(entry) = subscriptions.get_mut(&subscription) {
                            (*entry).push(repo_root);
                            continue;
                        }
                    }
                    subscriptions.insert(subscription, vec![repo_root]);
                }
            } else {
                info!(
                    "Skipping the file '{}' because format is invalid",
                    path.display()
                );
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
    return Ok(subscriptions);
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

pub fn read_access_token(user_token_path: &Option<PathBuf>) -> Result<Token> {
    // Try to read OAuth token from a file.
    let token = if let &Some(ref user_token_path) = user_token_path {
        let mut user_token_path = user_token_path.clone();
        user_token_path.push(TOKEN_FILENAME);
        info!(
            "Token Lookup: reading commitcloud OAuth token from a file {}...",
            user_token_path.display()
        );
        match fs::OpenOptions::new().read(true).open(user_token_path) {
            Ok(ref mut file) => Ini::read_from(&mut io::BufReader::new(file))?
                .get_from(Some("commitcloud"), "user_token")
                .map(|s| s.trim().to_string()),
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(err) => {
                error!("{}", err);
                bail!(err)
            }
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
    let clicat_tool = if hostcaps::is_prod() {
        "clicat"
    } else {
        "corp_clicat"
    };
    info!(
        "Token Lookup: generating commitcloud CAT token via {}...",
        clicat_tool
    );
    let ten_mins_seconds = 600;
    let output = Command::new(clicat_tool)
        .args(vec![
            "create",
            "--verifier_type",
            "SERVICE_IDENTITY",
            "--verifier_id",
            "commitcloud-service",
            "--token_timeout_seconds",
            &ten_mins_seconds.to_string(),
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
