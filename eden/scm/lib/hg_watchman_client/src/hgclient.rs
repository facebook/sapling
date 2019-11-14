/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use failure::Fallible as Result;
use serde_json::json;
use std::path::{Path, PathBuf};
use watchman_client::queries::*;
use watchman_client::transport::Transport;

/// This module is a container for mercurial requests

pub struct HgWatchmanClient<T> {
    pub transport: T,
    pub repo_path: PathBuf,
}

impl<T> HgWatchmanClient<T>
where
    T: Transport,
{
    /// construct HgWatchmanClient for the given repo_path and transport

    pub fn new<P: AsRef<Path>>(transport: T, repo_path: P) -> HgWatchmanClient<T> {
        HgWatchmanClient {
            transport,
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    /// REQUESTS

    /// Ask watchman to start following the files in the repo_path

    pub fn watch_project(&mut self) -> Result<WatchProjectResponse> {
        self.transport.watch_project(&self.repo_path)
    }

    /// Query watchman for all directories in the working copy the since last known clock
    /// The results should be passed to treedirstate

    pub fn query_dirs(
        &mut self,
        sync_timeout: Option<u32>,             // should come from config
        empty_on_fresh_instance: Option<bool>, // should come from config
        clock: Option<String>,
    ) -> Result<QueryResponse> {
        self.transport.query(
            QueryRequestParams {
                sync_timeout,
                empty_on_fresh_instance,
                fields: Some(vec!["name"]),
                expression: Some(json!([
                    "allof",
                    ["type", "d"],
                    [
                        "not",
                        ["anyof", ["dirname", ".hg"], ["name", ".hg", "wholename"]],
                    ],
                ])),
                since: Some(clock.unwrap_or("c:0:0".into())),
                ..Default::default()
            },
            &self.repo_path,
        )
    }

    /// Query watchman for all files in the working copy since the last known clock
    /// The results should be passed to treedirstate

    pub fn query_files(
        &mut self,
        sync_timeout: Option<u32>,             // should come from config
        empty_on_fresh_instance: Option<bool>, // should come from config
        clock: Option<String>,
    ) -> Result<QueryResponse> {
        self.transport.query(
            QueryRequestParams {
                sync_timeout,
                empty_on_fresh_instance,
                fields: Some(vec!["mode", "mtime", "size", "exists", "name"]),
                expression: Some(json!([
                    "not",
                    ["anyof", ["dirname", ".hg"], ["name", ".hg", "wholename"]],
                ])),
                since: Some(clock.unwrap_or("c:0:0".into())),
                ..Default::default()
            },
            &self.repo_path,
        )
    }

    /// Dispatching the state-enter and state-leave signals for hg.filemerge to the watchman service

    pub fn state_filemerge_enter<P: AsRef<Path>>(&mut self, path: P) -> Result<StateEnterResponse>
    where
        P: serde::Serialize,
    {
        let params = StateEnterParams {
            name: Some("hg.filemerge".into()),
            metadata: Some(json!({ "path": path })),
        };
        self.transport.state_enter(params, &self.repo_path)
    }

    pub fn state_filemerge_leave<P: AsRef<Path>>(&mut self, path: P) -> Result<StateLeaveResponse>
    where
        P: serde::Serialize,
    {
        let params = StateLeaveParams {
            name: Some("hg.filemerge".into()),
            metadata: Some(json!({ "path": path })),
        };
        self.transport.state_leave(params, &self.repo_path)
    }

    /// Dispatching the state-enter and state-leave signals for other states
    /// like hg.update and hg.transaction

    pub fn state_enter(
        &mut self,
        state_name: &'static str,
        state_meta: serde_json::Value,
    ) -> Result<StateEnterResponse> {
        let params = StateEnterParams {
            name: Some(state_name.into()),
            metadata: Some(state_meta),
        };
        self.transport.state_enter(params, &self.repo_path)
    }

    pub fn state_leave(
        &mut self,
        state_name: &'static str,
        state_meta: serde_json::Value,
    ) -> Result<StateLeaveResponse> {
        let params = StateLeaveParams {
            name: Some(state_name.into()),
            metadata: Some(state_meta),
        };
        self.transport.state_leave(params, &self.repo_path)
    }
}
