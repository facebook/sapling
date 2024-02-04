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
use gix_actor::Signature;
use gix_object::Commit;
use gix_object::WriteTo;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;

use crate::upload_non_blob_git_object;
use crate::MappedGitCommitId;
use crate::TreeHandle;

fn get_signature(id_str: &str, time: &DateTime) -> Result<Signature> {
    let (name, email) = get_name_and_email(id_str)?;
    let signature_time = gix_date::Time::new(time.timestamp_secs(), time.tz_offset_secs());
    Ok(Signature {
        name: name.into(),
        email: email.into(),
        time: signature_time,
    })
}

fn get_name_and_email<'a>(input: &'a str) -> Result<(&'a str, &'a str)> {
    let regex = regex::Regex::new(r"((?<name>.*)<(?<email>.*)>)|(?<name_without_email>.*)")
        .context("Invalid regex for parsing name and email")?;
    let captures = regex
        .captures(input)
        .ok_or_else(|| anyhow::anyhow!("The name and email does not match regex"))?;
    let name = captures
        .name("name")
        .or(captures.name("name_without_email"))
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
        let commit_tree_id = gix_hash::oid::try_from_bytes(tree_handle.oid().as_ref())
            .with_context(|| {
                format_err!(
                    "Failure while converting Git hash {} into Git Object ID",
                    tree_handle.oid()
                )
            })?;
        let commit_parent_ids = parents
            .into_iter()
            .map(|c| {
                gix_hash::oid::try_from_bytes(c.oid().as_ref())
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
        // commit has no committer, then use an empty Git signature. This way converting the git commit back to
        // Mononoke would maintain a valid mapping.
        // NOTE: If either the committer name OR date are empty, then the committer is assumed to be empty.
        let committer = if let (Some(committer), Some(committer_date)) =
            (bonsai.committer(), bonsai.committer_date())
        {
            get_signature(committer, committer_date)?
        } else {
            Signature::default()
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
        let oid = gix_hash::oid::try_from_bytes(git_hash.as_ref()).with_context(|| {
            format_err!(
                "Failure while converting hash {} into Git Object Id",
                git_hash
            )
        })?;
        // Store the converted Git commit
        upload_non_blob_git_object(ctx, &derivation_ctx.blobstore(), oid, raw_commit_bytes).await?;
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

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::str::FromStr;

    use anyhow::format_err;
    use blobstore::Loadable;
    use bonsai_git_mapping::BonsaiGitMappingRef;
    use bookmarks::BookmarkKey;
    use bookmarks::BookmarksRef;
    use changesets::ChangesetsRef;
    use derived_data::BonsaiDerived;
    use fbinit::FacebookInit;
    use fixtures::TestRepoFixture;
    use futures_util::stream::TryStreamExt;
    use mononoke_types::hash::GitSha1;
    use repo_blobstore::RepoBlobstoreArc;
    use repo_derived_data::RepoDerivedDataRef;

    use super::*;
    use crate::fetch_non_blob_git_object;

    async fn compare_commits(
        repo: &(impl RepoBlobstoreArc + BonsaiGitMappingRef),
        ctx: &CoreContext,
        bonsai_commit_id: ChangesetId,
        git_commit_id: GitSha1,
    ) -> Result<()> {
        let blobstore = repo.repo_blobstore();
        let git_hash =
            gix_hash::oid::try_from_bytes(git_commit_id.as_ref()).with_context(|| {
                format_err!(
                    "Failure while converting hash {:?} into Git ObjectId.",
                    git_commit_id.to_hex()
                )
            })?;
        let bonsai_commit = bonsai_commit_id.load(ctx, blobstore).await?;
        let git_commit = fetch_non_blob_git_object(ctx, blobstore, git_hash)
            .await?
            .into_commit();
        // Validate that the parents match
        let bonsai_parent_set: HashSet<ChangesetId> = HashSet::from_iter(bonsai_commit.parents());
        assert_eq!(bonsai_parent_set.len(), git_commit.parents.len());
        for git_parent in git_commit.parents {
            let parent_csid = repo
                .bonsai_git_mapping()
                .get_bonsai_from_git_sha1(ctx, GitSha1::from_bytes(git_parent.as_slice())?)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Couldn't find bonsai changeset for git commit {}",
                        git_parent.to_hex()
                    )
                })?;
            assert!(
                bonsai_parent_set.contains(&parent_csid),
                "Parent of Git commit {:?} and Bonsai changeset {:?} do not match",
                git_commit_id.to_hex(),
                bonsai_commit_id
            );
        }
        // Validate the commit message matches
        assert_eq!(
            bonsai_commit.message(),
            git_commit.message.to_string().as_str()
        );
        // Validate the author signature matches
        let bonsai_author = get_signature(bonsai_commit.author(), bonsai_commit.author_date())?;
        assert_eq!(bonsai_author, git_commit.author);
        // Validate the committer signature matches
        let bonsai_committer = if let (Some(committer), Some(committer_date)) =
            (bonsai_commit.committer(), bonsai_commit.committer_date())
        {
            get_signature(committer, committer_date)?
        } else {
            Signature::default()
        };
        assert_eq!(bonsai_committer, git_commit.committer);
        Ok(())
    }

    /// This function generates Git commits for each bonsai commit in the fixture starting from
    /// the fixture's master Bonsai bookmark. It then checks that the Git commit and the Bonsai commit
    /// represent the same data.
    async fn run_commit_derivation_for_fixture(
        fb: FacebookInit,
        repo: impl BookmarksRef
        + RepoBlobstoreArc
        + RepoDerivedDataRef
        + ChangesetsRef
        + BonsaiGitMappingRef
        + Send
        + Sync,
    ) -> Result<(), anyhow::Error> {
        let ctx = CoreContext::test_mock(fb);

        let bcs_id = repo
            .bookmarks()
            .get(ctx.clone(), &BookmarkKey::from_str("master")?)
            .await?
            .ok_or_else(|| format_err!("no master"))?;

        // Validate that the derivation of the Git commit was successful
        MappedGitCommitId::derive(&ctx, &repo, bcs_id).await?;
        // All the generated git commit IDs would be stored in BonsaiGitMapping. For all such commits, validate
        // parity with its Bonsai counterpart.
        repo.changesets()
            .list_enumeration_range(&ctx, 0, u64::MAX, None, false)
            .try_filter_map(|(bcs_id, _)| {
                let repo = &repo;
                let ctx: &CoreContext = &ctx;
                async move {
                    match repo
                        .bonsai_git_mapping()
                        .get_git_sha1_from_bonsai(ctx, bcs_id.clone())
                        .await?
                    {
                        Some(git_sha1) => Ok(Some((bcs_id, git_sha1))),
                        None => Ok(None),
                    }
                }
            })
            .map_ok(|(bcs_id, git_sha1)| {
                let repo = &repo;
                let ctx = &ctx;
                async move { compare_commits(repo, ctx, bcs_id, git_sha1).await }
            })
            .try_buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?;
        Ok(())
    }

    macro_rules! impl_test {
        ($test_name:ident, $fixture:ident) => {
            #[fbinit::test]
            fn $test_name(fb: FacebookInit) -> Result<(), anyhow::Error> {
                let runtime = tokio::runtime::Runtime::new()?;
                runtime.block_on(async move {
                    let repo = fixtures::$fixture::getrepo(fb).await;
                    run_commit_derivation_for_fixture(fb, repo).await
                })
            }
        };
    }

    impl_test!(linear, Linear);
    impl_test!(branch_even, BranchEven);
    impl_test!(branch_uneven, BranchUneven);
    impl_test!(branch_wide, BranchWide);
    impl_test!(merge_even, MergeEven);
    impl_test!(many_files_dirs, ManyFilesDirs);
    impl_test!(merge_uneven, MergeUneven);
    impl_test!(unshared_merge_even, UnsharedMergeEven);
    impl_test!(unshared_merge_uneven, UnsharedMergeUneven);
    impl_test!(many_diamonds, ManyDiamonds);
}
