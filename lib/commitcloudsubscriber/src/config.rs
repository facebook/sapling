use std::path::PathBuf;

mod defaults {
    use std::path::PathBuf;

    pub fn joined_pool_path() -> Option<PathBuf> {
        ::std::env::home_dir().map(|mut dir| {
            dir.push(".commitcloud");
            dir.push("joined");
            PathBuf::from(dir)
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn user_token_path() -> Option<PathBuf> {
        ::std::env::home_dir().map(|mut dir| {
            dir.push(".commitcloudrc");
            PathBuf::from(dir)
        })
    }

    // macos default - keychain
    // but still can be overridden in the config with a path
    #[cfg(target_os = "macos")]
    pub fn user_token_path() -> Option<PathBuf> {
        None
    }

    pub fn cloudsync_retries() -> u32 {
        2
    }
}

/// Struct for decoding Commit Cloud configuration from TOML.
/// Each field has default implementation, meaning that it doesn't have to be present in TOML.

#[derive(Debug, Deserialize)]
pub struct CommitCloudConfig {
    /// Http endpoint for Commit Cloud requests
    #[serde(default)]
    pub interngraph_url: Option<String>,

    /// Server-Sent Events endpoint for Commit Cloud Live Notifications
    #[serde(default)]
    pub streaminggraph_url: Option<String>,

    /// Path to the directory containing list of current connected 'subscribers'
    /// This should be in sync with `hg cloud join` and `hg cloud leave`
    /// The idea is that hg is responsible for adding/removing 'subscriber' into this folder when necessary
    /// 'subscriber' is a simple ini file containing repo_name, repo_root and workspace
    /// Filename for a 'subscriber' can be any, just make it unique
    #[serde(default = "defaults::joined_pool_path")]
    pub joined_pool_path: Option<PathBuf>,

    /// Path to the file with OAuth token
    /// that is valid for Commit Cloud Live Notifications and
    /// Commit Cloud requests (optional)
    #[serde(default = "defaults::user_token_path")]
    pub user_token_path: Option<PathBuf>,

    /// Number of retries when we trigger `hg cloud sync`
    #[serde(default = "defaults::cloudsync_retries")]
    pub cloudsync_retries: u32,
}
