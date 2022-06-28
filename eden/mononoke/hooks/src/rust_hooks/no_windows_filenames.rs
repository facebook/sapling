/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use regex::bytes::Regex;

#[derive(Default)]
pub struct NoWindowsFilenamesBuilder<'a> {
    /// Paths on which bad Windows filenames are not disallowed.
    allowed_paths: Option<&'a str>,
}

impl<'a> NoWindowsFilenamesBuilder<'a> {
    pub fn set_from_config(mut self, config: &'a HookConfig) -> Self {
        if let Some(v) = config.strings.get("allowed_paths") {
            self = self.allowed_paths(v)
        }

        self
    }

    pub fn allowed_paths(mut self, regex: &'a str) -> Self {
        self.allowed_paths = Some(regex);
        self
    }

    pub fn build(self) -> Result<NoWindowsFilenames> {
        Ok(NoWindowsFilenames {
            allowed_paths: self
                .allowed_paths
                .map(Regex::new)
                .transpose()
                .context("Failed to create allowed_paths regex")?,
        })
    }
}

pub struct NoWindowsFilenames {
    allowed_paths: Option<Regex>,
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
impl NoWindowsFilenames {
    pub fn builder<'a>() -> NoWindowsFilenamesBuilder<'a> {
        NoWindowsFilenamesBuilder::default()
    }
}

const BAD_WINDOWS_PATH_ELEMENT_REGEX: &str =
    r#"(^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$"#;

fn check_path_for_bad_elements(path: &MPath) -> Result<HookExecution, Error> {
    let bad_windows_path_element = Regex::new(BAD_WINDOWS_PATH_ELEMENT_REGEX)?;
    for element in path {
        if bad_windows_path_element.is_match(element.as_ref()) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Illegal windows filename",
                format!("ABORT: Illegal windows filename: {}", element),
            )));
        }
    }
    Ok(HookExecution::Accepted)
}

#[async_trait]
impl FileHook for NoWindowsFilenames {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _context_fetcher: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
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

        if let Some(allowed_paths) = &self.allowed_paths {
            if allowed_paths.is_match(&path.to_vec()) {
                return Ok(HookExecution::Accepted);
            }
        }

        Ok(check_path_for_bad_elements(path)?)
    }
}

/// This test is only testing the `check_path_for_bad_elements`, the rest of the
/// hook is only tested through integration tests.
#[cfg(test)]
mod test {
    use super::*;

    fn check_path(path: &str) -> bool {
        match check_path_for_bad_elements(&MPath::new(&path).unwrap()).unwrap() {
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
