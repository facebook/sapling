/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{FileContentFetcher, FileHook, HookExecution, HookRejectionInfo};
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use maplit::hashset;
use mononoke_types::{FileChange, MPath};
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
        content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        path: &'path MPath,
    ) -> Result<HookExecution, Error> {
        let change = match change {
            None => return Ok(HookExecution::Accepted),
            Some(change) => change,
        };

        let mut filename_iter = path.basename().as_ref().rsplit(|c| *c == b'.');
        let suffix = filename_iter.next().expect("File without a name");
        if filename_iter.next().is_some() && self.allowed_suffixes.contains(suffix) {
            return Ok(HookExecution::Accepted);
        }

        let text = content_fetcher
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
