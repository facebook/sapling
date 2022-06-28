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
use maplit::hashset;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use std::collections::HashSet;

pub struct ConflictMarkers {
    allowed_suffixes: HashSet<&'static [u8]>,
}

impl ConflictMarkers {
    pub fn new() -> Self {
        Self {
            allowed_suffixes: hashset! {b"rst" as &[u8], b"markdown" as &[u8], b"md" as &[u8], b"rdoc" as &[u8]},
        }
    }
}

#[async_trait]
impl FileHook for ConflictMarkers {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        let change = match change {
            Some(change) => change,
            None => return Ok(HookExecution::Accepted),
        };

        let mut filename_iter = path.basename().as_ref().rsplit(|c| *c == b'.');
        let suffix = filename_iter.next().expect("File without a name");
        if filename_iter.next().is_some() && self.allowed_suffixes.contains(suffix) {
            return Ok(HookExecution::Accepted);
        }

        let text = content_manager
            .get_file_text(ctx, change.content_id())
            .await?;
        if let Some(text) = text {
            for line in text.as_ref().split(|c| *c == b'\r' || *c == b'\n') {
                if line.starts_with(b">>>>>>> ") || line.starts_with(b"<<<<<<< ") {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Conflict markers found",
                        format!("Conflict markers were found in file '{}'", path),
                    )));
                }
            }
        }
        Ok(HookExecution::Accepted)
    }
}
