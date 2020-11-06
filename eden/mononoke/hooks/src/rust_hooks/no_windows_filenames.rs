/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{CrossRepoPushSource, FileContentFetcher, FileHook, HookExecution, HookRejectionInfo};

use anyhow::{Context, Result};
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::{FileChange, MPath};
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
            bad_windows_path_element: Regex::new(r"^(?i)(((com|lpt)\d$)|(con|prn|aux|nul))($|\.)")?,
        })
    }
}

pub struct NoWindowsFilenames {
    allowed_paths: Option<Regex>,
    bad_windows_path_element: Regex,
}

/// Hook to disallow bad Windows filenames from being pushed.
///
/// These bad filenames are described by Microsoft as:
///  "CON, PRN, AUX, NUL, COM1, COM2, COM3, COM4, COM5, COM6, COM7, COM8, COM9, LPT1, LPT2, LPT3,
///  LPT4, LPT5, LPT6, LPT7, LPT8, and LPT9. Also avoid these names followed immediately by an
///  extension; for example, NUL.txt is not recommended. For more information, see Namespaces."
impl NoWindowsFilenames {
    pub fn builder<'a>() -> NoWindowsFilenamesBuilder<'a> {
        NoWindowsFilenamesBuilder::default()
    }
}

#[async_trait]
impl FileHook for NoWindowsFilenames {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _context_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        path: &'path MPath,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<HookExecution> {
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

        for element in path {
            if self.bad_windows_path_element.is_match(element.as_ref()) {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Illegal windows filename",
                    format!("ABORT: Illegal windows filename: {}", element),
                )));
            }
        }

        Ok(HookExecution::Accepted)
    }
}
