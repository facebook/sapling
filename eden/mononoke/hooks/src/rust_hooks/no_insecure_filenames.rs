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
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use regex::Regex;

pub struct NoInsecureFilenames {
    illegal_regex: Regex,
    insecure_lower_regex: Regex,
    insecure_regex: Vec<Regex>,
}

impl NoInsecureFilenames {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            // .hgtags files break remotefilelog and .hgsub/.hgsubstate break hg-git,
            illegal_regex: Regex::new(r"(^.hg(tags|sub|substate)$)")?,
            // Block commits that attempt to exploit CVE-2015-9390:
            // http://git-blame.blogspot.com.es/2014/12/git-1856-195-205-214-and-221-and.html.
            // There are three vulnerabilities here:
            // 1. files like .Git/config (on all case-insensitive file systems)
            // 2. files like .GIT~1/config (on Windows)
            // 3. files containing one out of 16 UTF-8 encoded Unicode characters that are ignorable (on OS X)
            //
            // Also block the .hg and .svn equivalents.
            insecure_lower_regex: Regex::new(r"(^|/)\.(git|hg|svn)((8b6c)?\~[0-9]+)?(/|$)")?,
            insecure_regex: vec![
                Regex::new(
                    r"\x{200b}|\x{200c}|\x{200d}|\x{200e}|\x{200f}|\x{202a}|\x{202b}|\x{202c}0",
                )?,
                Regex::new(
                    r"\x{202d}|\x{202e}|\x{206a}|\x{206b}|\x{206c}|\x{206d}|\x{206e}|\x{feff}",
                )?,
            ],
        })
    }
}

#[async_trait]
impl FileHook for NoInsecureFilenames {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if change.is_none() {
            return Ok(HookExecution::Accepted);
        }

        let path = format!("{}", path);
        let lower_path = path.to_lowercase();
        if self.illegal_regex.is_match(&lower_path) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Illegal filename",
                format!("ABORT: Illegal filename: {}", path),
            )));
        }

        if self.insecure_lower_regex.is_match(&lower_path) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Illegal insecure name",
                format!("ABORT: Illegal insecure name: {}", path),
            )));
        }

        for insecure_regex in &self.insecure_regex {
            if insecure_regex.is_match(&path) {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Illegal insecure name",
                    format!("ABORT: Illegal insecure name: {}", path),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}
