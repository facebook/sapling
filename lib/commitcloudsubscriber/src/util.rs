use error::*;
use ini::Ini;
use std::{fs, io};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use subscriber::Subscription;

#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::str;

static JOINED_DIR: &str = ".commitcloud";
static JOINED: &str = "joined";

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
                            repo_root.display(),
                            path.display()
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

pub fn read_access_token(user_token_path: &Option<PathBuf>) -> Result<String> {
    // try to read token from file
    let token = if let &Some(ref user_token_path) = user_token_path {
        let mut user_token_path = user_token_path.clone();
        user_token_path.push(TOKEN_FILENAME);
        info!(
            "Reading commitcloud OAuth token from a file {}...",
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
    // try to read token from keychain
    #[cfg(target_os = "macos")]
    {
        if token.is_none() {
            // security find-generic-password -g -s commitcloud -a commitcloud -w
            info!("Reading commitcloud OAuth token from keychain...");
            let output = Command::new("security")
                .args(vec![
                    "find-generic-password",
                    "-g",
                    "-s",
                    "commitcloud",
                    "-a",
                    "commitcloud",
                    "-w",
                ])
                .output()?;
            if !output.status.success() {
                error!("Process exited with: {}", output.status);
                bail!("Failed to retrieve token from keychain")
            }
            let token = str::from_utf8(&output.stdout)?.trim().to_string();
            if token.is_empty() {
                error!("Token is not found in the keychain");
                bail!("Token is not found in the keychain")
            } else {
                info!("Token is found in the keychain");
                return Ok(token);
            }
        }
    }
    #[cfg(unix)]
    {
        // try to read token from secrets tool
        if token.is_none() {
            // try to read from secrets_tool
            info!("Reading commitcloud Oauth token from secrets_tool...");
            let user = env::var("USER")?.to_uppercase();
            let key = format!("COMMITCLOUD_{}", user);
            let output = Command::new("secrets_tool")
                .args(vec!["get", key.as_str()])
                .output()?;
            if !output.status.success() {
                error!("Process exited with: {}", output.status);
                bail!("Failed to retrieve token from secrets using key {}", key)
            }
            let token = str::from_utf8(&output.stdout)?.trim().to_string();
            if token.is_empty() {
                error!("Token not found in secrets");
                bail!("Token not found in secrets");
            } else {
                info!("Token is found in secrets");
                return Ok(token);
            }
        }
    }
    token.ok_or(ErrorKind::CommitCloudUnexpectedError("token not found".into()).into())
}
