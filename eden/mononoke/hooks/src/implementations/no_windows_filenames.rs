/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::bytes::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

const BAD_WINDOWS_PATH_ELEMENT_REGEX: &str =
    r#"(^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$"#;

#[derive(Debug, Deserialize, Clone)]
pub struct NoWindowsFilenamesConfig {
    /// Paths on which bad Windows filenames are not disallowed.
    #[serde(default, with = "serde_regex")]
    allowed_paths: Option<Regex>,
    /// Message to include in the hook rejection if the file path matches the illegal pattern,
    /// with the following replacements
    /// ${filename} => The path of the file along with the filename
    /// ${illegal_pattern} => The illegal regex pattern that was matched
    illegal_filename_message: String,
}

/// Hook to disallow bad Windows filenames from being pushed.
///
/// These bad filenames are described by Microsoft as:
///  "CON, PRN, AUX, NUL, COM1, COM2, COM3, COM4, COM5, COM6, COM7, COM8, COM9, LPT1, LPT2, LPT3,
///  LPT4, LPT5, LPT6, LPT7, LPT8, and LPT9. Also avoid these names followed immediately by an
///  extension; for example, NUL.txt is not recommended. For more information, see Namespaces."
///
///  In addition the following characters are invalid: <>:"/\|?* and the chars 0-31
///
///  The filename shouldn't end with space or period. Windows shell and UX don't support such files.
///
///  More info: https://docs.microsoft.com/en-gb/windows/win32/fileio/naming-a-file
#[derive(Clone, Debug)]
pub struct NoWindowsFilenamesHook {
    config: NoWindowsFilenamesConfig,
}

impl NoWindowsFilenamesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        config.parse_options().map(Self::with_config)
    }

    pub fn with_config(config: NoWindowsFilenamesConfig) -> Self {
        Self { config }
    }

    fn check_path_for_bad_elements(&self, path: &NonRootMPath) -> Result<HookExecution, Error> {
        let bad_windows_path_element = Regex::new(BAD_WINDOWS_PATH_ELEMENT_REGEX)
            .context("Error while creating bad windows path element regex")?;
        for element in path {
            if bad_windows_path_element.is_match(element.as_ref()) {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Illegal windows filename",
                    self.config
                        .illegal_filename_message
                        .replace("${filename}", &path.to_string())
                        .replace("${illegal_pattern}", &bad_windows_path_element.to_string()),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}

#[async_trait]
impl FileHook for NoWindowsFilenamesHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _context_fetcher: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected pushes we rely on the hook
            // running in the original repo
            return Ok(HookExecution::Accepted);
        }

        if change.is_none() {
            return Ok(HookExecution::Accepted);
        }

        if let Some(allowed_paths) = &self.config.allowed_paths {
            if allowed_paths.is_match(&path.to_vec()) {
                return Ok(HookExecution::Accepted);
            }
        }

        self.check_path_for_bad_elements(path)
    }
}

/// This test is only testing the `check_path_for_bad_elements`, the rest of the
/// hook is only tested through integration tests.
#[cfg(test)]
mod test {
    use super::*;

    fn check_path(path: &str) -> bool {
        let hook = NoWindowsFilenamesHook::with_config(NoWindowsFilenamesConfig {
            allowed_paths: None,
            illegal_filename_message: "hook failed".to_string(),
        });
        match hook
            .check_path_for_bad_elements(&NonRootMPath::new(path).unwrap())
            .unwrap()
        {
            HookExecution::Accepted => true,
            HookExecution::Rejected(_) => false,
        }
    }

    #[test]
    fn test_good_paths() {
        assert!(check_path("dir/some_filename.exe"));
        assert!(check_path(
            "very_very_very_very_very_very_very_very_very_very_very_very_very_very_very_very_long_filename"
        ));
        assert!(check_path("aaa/LPT2137"));
        assert!(check_path("COM"));
        assert!(check_path("spaces are allowed!"));
        assert!(check_path("x/y/z/file_with_tildle~_in_the_name"));
    }

    #[test]
    fn invalid_chars() {
        assert!(!check_path("x/y/z/file_with_backslash\\_in_the_name"));
        assert!(!check_path("x/y/dir_with_pipe|in_the_name/file"));
        assert!(!check_path("x/y/dir_with_less_than<in_the_name/file"));
    }

    #[test]
    fn invalid_names() {
        assert!(!check_path("aaa/COm1"));
        assert!(!check_path("aaa/lPt3"));
        assert!(!check_path("x/CON/file"));
        assert!(!check_path("x/y/AUX"));
        assert!(!check_path("NUL.txt"));
        assert!(!check_path("COM1.txt"));
    }
}
