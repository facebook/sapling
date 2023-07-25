/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use git_actor::Signature;
use git_object::Commit;
use git_object::WriteTo;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;

use crate::upload_git_object;
use crate::MappedGitCommitId;
use crate::TreeHandle;

fn get_signature(id_str: &str, time: &DateTime) -> Result<Signature> {
    let (name, email) = get_name_and_email(id_str)?;
    let signature_time = git_actor::Time::new(time.timestamp_secs() as u32, time.tz_offset_secs());
    Ok(Signature {
        name: name.into(),
        email: email.into(),
        time: signature_time,
    })
}

fn get_name_and_email<'a>(input: &'a str) -> Result<(&'a str, &'a str)> {
    let regex = regex::Regex::new(r"(?<name>.*)<(?<email>.*)>")
        .context("Invalid regex for parsing name and email")?;
    let captures = regex
        .captures(input)
        .ok_or_else(|| anyhow::anyhow!("The name and email does not match regex"))?;
    let name = captures
        .name("name")
        .ok_or_else(|| anyhow::anyhow!("The name cannot be empty"))?
        .as_str();
    let email = captures.name("email").map_or("", |m| m.as_str()); // The email can be empty
    Ok((name, email))
}

#[async_trait]
impl BonsaiDerivable for MappedGitCommitId {
    const VARIANT: DerivableType = DerivableType::GitCommit;

    type Dependencies = dependencies![TreeHandle];

    /// Derives a Git commit for a given Bonsai changeset. The mapping is recorded in bonsai_git_mapping and as a result
    /// imported Mononoke commits from Git repos will by default be marked as having their Git commits derived. This method
    /// will only be invoked for commits that originate within Mononoke.
    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        if bonsai.is_snapshot() {
            bail!("Can't derive MappedGitCommitId for snapshot")
        }
        let tree_handle = derivation_ctx
            .derive_dependency::<TreeHandle>(ctx, bonsai.get_changeset_id())
            .await?;
        let commit_tree_id = git_hash::oid::try_from_bytes(tree_handle.oid().as_ref())
            .with_context(|| {
                format_err!(
                    "Failure while converting Git hash {} into Git Object ID",
                    tree_handle.oid()
                )
            })?;
        let commit_parent_ids = parents
            .into_iter()
            .map(|c| {
                git_hash::oid::try_from_bytes(c.oid().as_ref())
                    .with_context(|| {
                        format_err!(
                            "Failure while converting Git hash {} into Git Object ID",
                            c.oid()
                        )
                    })
                    .map(|oid| oid.into())
            })
            .collect::<Result<Vec<_>>>()?;
        let author = get_signature(bonsai.author(), bonsai.author_date())?;
        // Git always needs a committer whereas Mononoke may or may not have a separate committer. If the Mononoke
        // commit has no committer, then re-use the author as committer.
        // NOTE: If either the committer name OR date are empty, then the committer is assumed to be the author.
        let committer = if let (Some(committer), Some(committer_date)) =
            (bonsai.committer(), bonsai.committer_date())
        {
            get_signature(committer, committer_date)?
        } else {
            author.clone()
        };
        let git_commit = Commit {
            tree: commit_tree_id.into(),
            parents: commit_parent_ids.into(),
            author,
            committer,
            encoding: None, // always UTF-8 from Mononoke
            message: bonsai.message().into(),
            extra_headers: Vec::new(), // These are git specific headers. Will be empty for converted commits
        };
        // Convert the commit into raw bytes
        let mut raw_commit_bytes = git_commit.loose_header().into_vec();
        git_commit.write_to(raw_commit_bytes.by_ref())?;
        let git_hash = hash_bytes(Sha1IncrementalHasher::new(), raw_commit_bytes.as_slice());
        let oid = git_hash::oid::try_from_bytes(git_hash.as_ref()).with_context(|| {
            format_err!(
                "Failure while converting hash {} into Git Object Id",
                git_hash
            )
        })?;
        // Store the converted Git commit
        upload_git_object(ctx, &derivation_ctx.blobstore(), oid, raw_commit_bytes).await?;
        Ok(Self::new(git_hash.into()))
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        derivation_ctx
            .bonsai_git_mapping()?
            .add(
                ctx,
                BonsaiGitMappingEntry::new(self.oid().clone(), changeset_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let git_sha1 = derivation_ctx
            .bonsai_git_mapping()?
            .get_git_sha1_from_bonsai(ctx, changeset_id)
            .await?;
        Ok(git_sha1.map(Self::new))
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::commit_handle(
            thrift::DerivedDataCommitHandle::mapped_commit_id(id),
        ) = data
        {
            Self::try_from(id)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::commit_handle(
            thrift::DerivedDataCommitHandle::mapped_commit_id(data.into()),
        ))
    }
}

impl_bonsai_derived_via_manager!(MappedGitCommitId);
