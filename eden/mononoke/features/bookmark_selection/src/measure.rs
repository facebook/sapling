/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;

/// Bookmark tree reads issued at once. The rollup read is cheap, so this is about
/// not queueing a fleet sweep's worth of blobstore traffic, not about latency.
const CONCURRENCY: usize = 10;

/// A bookmark and the size of the tree it points at.
#[derive(Debug)]
pub struct Candidate {
    pub name: String,
    pub csid: ChangesetId,
    pub files_count: u64,
    pub files_total_size: u64,
}

/// Files and bytes at a changeset, read from the content manifest's rolled-up
/// descendant counts. Reading the rollup rather than walking the manifest is what
/// makes this O(1) per bookmark.
///
/// Returns `None` when the manifest has not been derived. Deliberately never
/// derives: the whole reason this is affordable is that the rollup is already
/// there, and derivation is not O(1). A sweep that derived on demand would cost
/// what walking the manifest costs, which is the thing being avoided.
async fn size_at(
    ctx: &CoreContext,
    repo: &(impl RepoDerivedDataRef + RepoBlobstoreRef),
    csid: ChangesetId,
) -> Result<Option<(u64, u64)>> {
    let Some(root) = repo
        .repo_derived_data()
        .fetch_derived::<RootContentManifestId>(ctx, csid)
        .await?
    else {
        return Ok(None);
    };

    let manifest = root
        .into_content_manifest_id()
        .load(ctx, repo.repo_blobstore())
        .await
        .with_context(|| format!("loading content manifest for {csid}"))?;
    let counts = manifest.subentries.rollup_data().descendant_counts;
    Ok(Some((counts.files_count, counts.files_total_size)))
}

/// Sizes every candidate bookmark. Bookmarks with no derived manifest are absent
/// from the result rather than counted as zero: a repo that could not be measured
/// must never be reported as empty, or every size bucket skews low while looking
/// healthy.
pub async fn measure(
    ctx: &CoreContext,
    repo: &(impl RepoDerivedDataRef + RepoBlobstoreRef + Sync),
    bookmarks: Vec<(String, ChangesetId)>,
) -> Result<Vec<Candidate>> {
    stream::iter(bookmarks)
        .map(|(name, csid)| async move {
            let sized = size_at(ctx, repo, csid).await?;
            Ok::<_, anyhow::Error>(sized.map(|(files_count, files_total_size)| Candidate {
                name,
                csid,
                files_count,
                files_total_size,
            }))
        })
        .buffer_unordered(CONCURRENCY)
        .try_filter_map(|candidate| async move { Ok(candidate) })
        .try_collect()
        .await
}

/// Orders candidates best first and returns the winner.
///
/// Ties are the common case, not an edge case: refs routinely point at equal
/// sized trees, and on a fleet sweep roughly half of repos have one. Without an
/// explicit tie-break the winner is whichever read happened to finish first, so
/// the recorded bookmark churns run to run while the size does not, which is
/// indistinguishable from the selection instability this metric exists to
/// remove. Largest tree wins; equal trees go to the lexicographically first name.
pub fn pick_winner(candidates: &mut [Candidate]) -> Option<&Candidate> {
    candidates.sort_by(|a, b| {
        b.files_total_size
            .cmp(&a.files_total_size)
            .then_with(|| a.name.cmp(&b.name))
    });
    candidates.first()
}

#[cfg(test)]
mod test {
    use derivation_queue_thrift::DerivationPriority;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;

    fn candidate(name: &str, files_total_size: u64) -> Candidate {
        Candidate {
            name: name.to_string(),
            csid: ChangesetId::from_bytes([0; 32]).unwrap(),
            files_count: 1,
            files_total_size,
        }
    }

    #[mononoke::test]
    fn test_largest_tree_wins() {
        let mut candidates = vec![candidate("a", 1), candidate("b", 3), candidate("c", 2)];
        assert_eq!(pick_winner(&mut candidates).unwrap().name, "b");
    }

    #[mononoke::test]
    fn test_tie_goes_to_first_name() {
        let mut candidates = vec![candidate("remotes/main", 5), candidate("heads/main", 5)];
        assert_eq!(pick_winner(&mut candidates).unwrap().name, "heads/main");
    }

    #[mononoke::test]
    fn test_no_candidates_no_winner() {
        assert!(pick_winner(&mut []).is_none());
    }

    #[mononoke::fbinit_test]
    async fn test_derived_changeset_is_sized(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a", "x")
            .add_file("dir/b", "yyyy")
            .commit()
            .await?;
        repo.repo_derived_data()
            .derive::<RootContentManifestId>(&ctx, cs, DerivationPriority::LOW)
            .await?;

        let sized = measure(&ctx, &repo, vec![("main".to_string(), cs)]).await?;
        assert_eq!(sized.len(), 1);
        assert_eq!(sized[0].name, "main");
        assert_eq!(sized[0].files_count, 2);
        assert_eq!(sized[0].files_total_size, 5);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_underived_changeset_is_absent(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a", "x")
            .commit()
            .await?;

        let sized = measure(&ctx, &repo, vec![("main".to_string(), cs)]).await?;
        assert!(sized.is_empty());
        Ok(())
    }
}
