// Copyright Facebook, Inc. 2019

use std::path::PathBuf;

use failure::Fallible;

use types::Key;

pub trait EdenApi {
    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    fn health_check(&self) -> Fallible<()>;

    /// Fetch the content of the specified file from the API server and write
    /// it to a datapack in the configured cache directory. Returns the path
    /// of the resulting packfile.
    fn get_files(&self, keys: impl IntoIterator<Item = Key>) -> Fallible<PathBuf>;

    /// Fetch the history of the specified file from the API server and write
    /// it to a historypack in the configured cache directory. Returns the path
    /// of the resulting packfile.
    fn get_history(
        &self,
        keys: impl IntoIterator<Item = Key>,
        max_depth: Option<u32>,
    ) -> Fallible<PathBuf>;
}
