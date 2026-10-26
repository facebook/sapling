/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `no_insecure_filenames` hook.
    pub(crate) struct NoInsecureFilenamesMessages for "no_insecure_filenames" {
        /// A changed file is a `.hgtags`, `.hgsub` or `.hgsubstate` file.
        illegal_filename(
            /// Path of the file, quoted, with special characters escaped.
            path,
        ) = "ABORT: Illegal filename: ${path}";
        /// A changed file's path could be used to attack a client, for
        /// example a path into `.git` on a case-insensitive file system.
        insecure_name(
            /// Path of the file, quoted, with special characters escaped.
            path,
        ) = "ABORT: Illegal insecure name: ${path}";
    }
}

pub struct NoInsecureFilenames {
    illegal_regex: Regex,
    insecure_lower_regex: Regex,
    insecure_regex: Vec<Regex>,
    messages: NoInsecureFilenamesMessages,
}

impl NoInsecureFilenames {
    pub fn new(config: &HookConfig) -> Result<Self, Error> {
        Ok(Self {
            messages: NoInsecureFilenamesMessages::new(config),
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
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if change.is_none() {
            return Ok(HookExecution::accepted());
        }

        let path = path.to_string();
        let lower_path = path.to_lowercase();
        if self.illegal_regex.is_match(&lower_path) {
            return Ok(HookExecution::rejected(
                self.messages
                    .illegal_filename(format!("{path:?}"))
                    .into_rejection("Illegal filename"),
            ));
        }

        if self.insecure_lower_regex.is_match(&lower_path) {
            return Ok(HookExecution::rejected(
                self.messages
                    .insecure_name(format!("{path:?}"))
                    .into_rejection("Illegal insecure name"),
            ));
        }

        for insecure_regex in &self.insecure_regex {
            if insecure_regex.is_match(&path) {
                return Ok(HookExecution::rejected(
                    self.messages
                        .insecure_name(format!("{path:?}"))
                        .into_rejection("Illegal insecure name"),
                ));
            }
        }
        Ok(HookExecution::accepted())
    }
}
