/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use std::path::PathBuf;

mod defaults {
    use std::path::PathBuf;

    /// Important! Have to be in sync with commitcloudutil.py
    /// Unix:
    ///    returns the value of the 'HOME' environment variable
    ///    if it is set and not equal to the empty string
    /// Windows:
    ///    returns the value of the 'APPDATA' environment variable
    ///    if it is set and not equal to the empty string

    fn home_dir_os() -> Option<PathBuf> {
        let var = {
            #[cfg(not(target_os = "windows"))]
            {
                "HOME"
            }
            #[cfg(target_os = "windows")]
            {
                "APPDATA"
            }
        };
        match ::std::env::var_os(var) {
            Some(ref val) if !val.is_empty() => Some(val),
            _ => None,
        }
        .map(|value| PathBuf::from(value))
    }

    pub fn connected_subscribers_path() -> Option<PathBuf> {
        home_dir_os()
    }

    pub fn user_token_path() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            None
        }
        #[cfg(not(target_os = "macos"))]
        {
            home_dir_os()
        }
    }

    pub fn cloudsync_retries() -> u32 {
        2
    }

    pub fn tcp_receiver_port() -> u16 {
        15432
    }

    pub fn alive_throttling_rate_sec() -> u64 {
        60 * 5
    }

    pub fn error_throttling_rate_sec() -> u64 {
        60 * 5
    }
}

/// Struct for decoding Commit Cloud configuration from TOML.
/// Each field has default implementation, meaning that it doesn't have to be present in TOML.

#[derive(Debug, Deserialize)]
pub struct CommitCloudConfig {
    /// Http endpoint for Commit Cloud requests
    #[serde(default)]
    pub service_url: Option<String>,

    /// Server-Sent Events endpoint for real-time Commit Cloud Notifications
    #[serde(default)]
    pub notification_url: Option<String>,

    /// Path to the directory containing current connected subscribers
    /// This is an optional override, see logic for the default location
    /// Subscriber is a simple ini file containing repo_name, repo_root and workspace
    /// Filename for a subscriber can be any, just make it unique
    /// Mercurial is responsible for adding/removing subscribers into this folder when necessary
    /// This should be in sync with `hg cloud join` and `hg cloud leave` commands
    /// Have to be in sync with 'connected_subscribers_path' option in mercurial config if defined
    #[serde(default = "defaults::connected_subscribers_path")]
    pub connected_subscribers_path: Option<PathBuf>,

    /// Path to the directory containing .commitcloudrc file with OAuth token
    /// that is valid for Server-Sent Events Commit Cloud endpoint and Http endpoint
    /// This is an optional override, see logic for the default location
    /// Have to be in sync with 'user_token_path' option in mercurial config if defined
    /// Macos default storage to token is keychain
    #[serde(default = "defaults::user_token_path")]
    pub user_token_path: Option<PathBuf>,

    /// Number of retries when we trigger `hg cloud sync`
    #[serde(default = "defaults::cloudsync_retries")]
    pub cloudsync_retries: u32,

    /// Tcp port to run a receiver
    /// This is a simple receiver working on tcp socket
    #[serde(default = "defaults::tcp_receiver_port")]
    pub tcp_receiver_port: u16,

    /// Throttling rate for logging alive notifications in sec
    #[serde(default = "defaults::alive_throttling_rate_sec")]
    pub alive_throttling_rate_sec: u64,

    /// Throttling rate for logging errors
    #[serde(default = "defaults::error_throttling_rate_sec")]
    pub error_throttling_rate_sec: u64,
}
