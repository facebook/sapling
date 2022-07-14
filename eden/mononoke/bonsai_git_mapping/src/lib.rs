/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql::Transaction;
use anyhow::Result;
use ascii::AsciiStr;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::hash::GitSha1;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use slog::warn;

mod errors;
mod sql;

pub use crate::errors::AddGitMappingErrorKind;
pub use crate::sql::SqlBonsaiGitMapping;
pub use crate::sql::SqlBonsaiGitMappingBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGitMappingEntry {
    pub git_sha1: GitSha1,
    pub bcs_id: ChangesetId,
}

impl BonsaiGitMappingEntry {
    pub fn new(git_sha1: GitSha1, bcs_id: ChangesetId) -> Self {
        BonsaiGitMappingEntry { git_sha1, bcs_id }
    }
}

pub enum BonsaisOrGitShas {
    Bonsai(Vec<ChangesetId>),
    GitSha1(Vec<GitSha1>),
}

impl BonsaisOrGitShas {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaisOrGitShas::Bonsai(v) => v.is_empty(),
            BonsaisOrGitShas::GitSha1(v) => v.is_empty(),
        }
    }
}

impl From<ChangesetId> for BonsaisOrGitShas {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaisOrGitShas::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaisOrGitShas {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaisOrGitShas::Bonsai(cs_ids)
    }
}

impl From<GitSha1> for BonsaisOrGitShas {
    fn from(git_sha1: GitSha1) -> Self {
        BonsaisOrGitShas::GitSha1(vec![git_sha1])
    }
}

impl From<Vec<GitSha1>> for BonsaisOrGitShas {
    fn from(revs: Vec<GitSha1>) -> Self {
        BonsaisOrGitShas::GitSha1(revs)
    }
}

#[facet::facet]
#[async_trait]
pub trait BonsaiGitMapping: Send + Sync {
    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind>;

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind>;

    async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrGitShas,
    ) -> Result<Vec<BonsaiGitMappingEntry>>;

    async fn get_git_sha1_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> Result<Option<GitSha1>> {
        let result = self
            .get(ctx, BonsaisOrGitShas::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.git_sha1))
    }

    async fn get_bonsai_from_git_sha1(
        &self,
        ctx: &CoreContext,
        git_sha1: GitSha1,
    ) -> Result<Option<ChangesetId>> {
        let result = self
            .get(ctx, BonsaisOrGitShas::GitSha1(vec![git_sha1]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn bulk_import_from_bonsai(
        &self,
        ctx: &CoreContext,
        changesets: &[BonsaiChangeset],
    ) -> Result<()> {
        let mut entries = vec![];
        for bcs in changesets.iter() {
            match extract_git_sha1_from_bonsai_extra(bcs.extra()) {
                Ok(Some(git_sha1)) => {
                    let entry = BonsaiGitMappingEntry::new(git_sha1, bcs.get_changeset_id());
                    entries.push(entry);
                }
                Ok(None) => {
                    warn!(
                        ctx.logger(),
                        "The git mapping is missing in bonsai commit extras: {:?}",
                        bcs.get_changeset_id()
                    );
                }
                Err(e) => {
                    warn!(ctx.logger(), "Couldn't fetch git mapping: {:?}", e);
                }
            }
        }
        self.bulk_add(ctx, &entries).await?;
        Ok(())
    }
}

pub const HGGIT_SOURCE_EXTRA: &str = "hg-git-rename-source";
pub const CONVERT_REVISION_EXTRA: &str = "convert_revision";

pub fn extract_git_sha1_from_bonsai_extra<'a, 'b, T>(extra: T) -> Result<Option<GitSha1>>
where
    T: Iterator<Item = (&'a str, &'b [u8])>,
{
    let (mut hggit_source_extra, mut convert_revision_extra) = (None, None);
    for (key, value) in extra {
        if key == HGGIT_SOURCE_EXTRA {
            hggit_source_extra = Some(value);
        }
        if key == CONVERT_REVISION_EXTRA {
            convert_revision_extra = Some(value);
        }
    }

    if hggit_source_extra == Some(b"git") {
        if let Some(convert_revision_extra) = convert_revision_extra {
            let git_sha1 = AsciiStr::from_ascii(convert_revision_extra)?;
            let git_sha1 = GitSha1::from_ascii_str(git_sha1)?;
            return Ok(Some(git_sha1));
        }
    }
    Ok(None)
}
