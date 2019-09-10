// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::path::decode_path_fallible;
use std::path::PathBuf;

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct FileName(#[serde(deserialize_with = "decode_path_fallible")] pub PathBuf);

/// Commands
pub const WATCH_PROJECT: &'static str = "watch-project";
pub const QUERY: &'static str = "query";
pub const GET_SOCKNAME: &'static str = "get-sockname";
pub const STATE_ENTER: &'static str = "state-enter";
pub const STATE_LEAVE: &'static str = "state-leave";

/// Types
/// (Deserialization will not fail for unknown fields)
/// These are minimun fields that are used by mercurial

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestResult<T> {
    Error(RequestError),
    Ok(T),
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct RequestError {
    /// error message if request has failed
    pub error: String,
    /// version of watchman daemon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// [watch-project](https://facebook.github.io/watchman/docs/cmd/watch-project.html)

#[derive(Clone, Default, Debug, Serialize)]
pub struct WatchProjectRequest(pub &'static str, pub PathBuf);

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct WatchProjectResponse {
    /// version of watchman daemon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// repo path that will be watched
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<FileName>,
    /// can be inotify, eden, etc
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watcher: Option<String>,
    /// relative path to the dir that has been watched
    /// files in query request will be returns wihout the prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<FileName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_validated: Option<bool>,
}

/// [query](https://facebook.github.io/watchman/docs/cmd/query.html)

#[derive(Clone, Default, Debug, Serialize)]
pub struct QueryRequestParams {
    /// "fields" should consists of subset of
    /// (default is ["name", "exists", "new", "size", "mode"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<&'static str>>,
    /// Enable the suffix generator as a source of files to filter.
    /// Specifying ["php"] will walk all files with the php suffix
    /// and then filter by the specified expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<Vec<PathBuf>>,
    /// expression Term
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<serde_json::Value>,
    /// syncronization timeout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_timeout: Option<u32>,
    /// when watchman is restarted, or a watch is deleted and re-added,
    /// the first trigger invocation includes every file matching the specified expression
    /// unless the option is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_on_fresh_instance: Option<bool>,
    /// watchman clock value from the previous request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct QueryRequest(pub &'static str, pub PathBuf, pub QueryRequestParams);

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct FileInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<FileName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<u64>,
}

pub type FileListNamesOnly = Vec<FileName>;
pub type FileListMultipleFields = Vec<FileInfo>;

/// If the field list is a single element,
/// then the result is an array of elements of the same type as the specified field.
/// Otherwise, it is an array of objects holding all of the specified fields.
/// In mercurial we only use names if we run single element request

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Files {
    FileListNamesOnly(FileListNamesOnly),
    FileListMultipleFields(FileListMultipleFields),
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct QueryResponse {
    /// version of watchman daemon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// list of files info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Files>,
    /// clock value at the point in time at which the results were generated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// is true if the particular clock value indicates that
    /// it was returned by a different instance of watchman, or that
    /// the filesystem was recrawled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_fresh_instance: Option<bool>,
}

/// [get-sockname](https://facebook.github.io/watchman/docs/cmd/get-sockname.html)

#[derive(Clone, Default, Debug, Serialize)]
pub struct GetSockNameRequest(pub (&'static str,));

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct GetSockNameResponse {
    /// version of watchman daemon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// socket name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sockname: Option<FileName>,
}

/// [state-enter](https://facebook.github.io/watchman/docs/cmd/state-enter.html)

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StateEnterParams {
    /// name of the state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// metadata (any json)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct StateEnterRequest(pub &'static str, pub PathBuf, pub StateEnterParams);

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct StateEnterResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<FileName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "state-enter")]
    pub state_enter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

/// [state-leave](https://facebook.github.io/watchman/docs/cmd/state-leave.html)

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StateLeaveParams {
    /// name of the state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// metadata (any json)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct StateLeaveRequest(pub &'static str, pub PathBuf, pub StateLeaveParams);

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct StateLeaveResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<FileName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "state-leave")]
    pub state_leave: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}
