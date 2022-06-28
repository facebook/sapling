/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use trait_alias::trait_alias;

use crate::commit_id::IdentityScheme;

#[trait_alias]
pub trait Repo =
    BonsaiHgMappingRef + BonsaiGitMappingRef + BonsaiGlobalrevMappingRef + BonsaiSvnrevMappingRef;

pub struct BookmarkLogEntry {
    timestamp: Timestamp,
    bookmark: BookmarkName,
    reason: BookmarkUpdateReason,
    ids: Vec<(IdentityScheme, String)>,
    bundle_id: Option<u64>,
}

impl BookmarkLogEntry {
    pub async fn new(
        ctx: &CoreContext,
        repo: &impl Repo,
        timestamp: Timestamp,
        bookmark: BookmarkName,
        reason: BookmarkUpdateReason,
        changeset_id: Option<ChangesetId>,
        bundle_id: Option<u64>,
        schemes: &[IdentityScheme],
    ) -> Result<Self> {
        let ids = if let Some(changeset_id) = changeset_id {
            stream::iter(schemes.iter().copied())
                .map(|scheme| {
                    Ok::<_, Error>(async move {
                        match scheme.map_commit_id(ctx, repo, changeset_id).await? {
                            Some(commit_id) => Ok(Some((scheme, commit_id))),
                            None => Ok(None),
                        }
                    })
                })
                .try_buffered(10)
                .try_filter_map(|commit_id| async move { Ok(commit_id) })
                .try_collect()
                .await?
        } else {
            Vec::new()
        };
        Ok(BookmarkLogEntry {
            timestamp,
            bookmark,
            reason,
            ids,
            bundle_id,
        })
    }
}

impl fmt::Display for BookmarkLogEntry {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if let Some(bundle_id) = self.bundle_id {
            write!(fmt, "{} ", bundle_id)?;
        }
        write!(fmt, "({})", self.bookmark)?;
        match self.ids.as_slice() {
            [] => {}
            [(_, id)] => write!(fmt, " {}", id)?,
            ids => {
                for (scheme, id) in ids {
                    write!(fmt, " {}={}", scheme.to_string(), id)?;
                }
            }
        }
        write!(
            fmt,
            " {} {}",
            self.reason,
            DateTime::from(self.timestamp).as_chrono().to_rfc3339()
        )?;
        Ok(())
    }
}
