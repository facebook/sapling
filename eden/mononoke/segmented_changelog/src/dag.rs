/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Result};
use futures::{
    future,
    stream::{self, StreamExt, TryStreamExt},
};

use dag::{self, Id as SegmentedChagnelogId, InProcessIdDag, Level};

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::{idmap::IdMap, parents::Parents};

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
#[derive(Debug)]
pub struct Dag {
    idmap: IdMap,
    iddag: InProcessIdDag,
}

impl Dag {
    pub fn new() -> Self {
        Dag {
            idmap: IdMap::new(),
            iddag: InProcessIdDag::new_in_process(),
        }
    }

    // Dummy method. A production setup would have the changeset built by a separate job.
    pub async fn build_up(
        &mut self,
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
    ) -> Result<()> {
        let high_scid = self.idmap.build_up(ctx, blob_repo, head).await?;
        let low_scid = self.iddag.next_free_id(0 as Level, high_scid.group())?;
        if low_scid >= high_scid {
            return Ok(());
        }
        let idmap = &self.idmap;

        let parents_fetcher = Parents::new(ctx, blob_repo);

        // TODO(sfilip): buffering
        let parents: Vec<Vec<SegmentedChagnelogId>> = stream::iter(low_scid.to(high_scid))
            .map(|scid| idmap.convert_scid(&scid))
            .and_then(|name: ChangesetId| parents_fetcher.get(name))
            .and_then(|names| {
                let scids = names
                    .iter()
                    .map(|name| idmap.convert_name(name))
                    .collect::<Result<Vec<_>>>();
                future::ready(scids)
            })
            .try_collect()
            .await?;
        // Note. IdMap fetches should be async so we want to batch them.
        let get_parents = |scid: SegmentedChagnelogId| {
            parents
                .get((scid.0 - low_scid.0) as usize)
                .map(|list| list.clone())
                .ok_or_else(|| {
                    format_err!(
                        "invalid Id requested by IdDag: {}; present Id range: [{}, {}]",
                        scid,
                        low_scid,
                        high_scid
                    )
                })
        };
        // TODO(sfilip): check return value from build_segments_volatile
        self.iddag
            .build_segments_volatile(high_scid, &get_parents)?;
        Ok(())
    }

    // TODO(sfilip): error scenarios
    pub async fn location_to_name(&self, known: ChangesetId, distance: u64) -> Result<ChangesetId> {
        let known_scid = self.idmap.convert_name(&known)?;
        let dist_ancestor_scid = self.iddag.first_ancestor_nth(known_scid, distance)?;
        let dist_ancestor = self.idmap.convert_scid(&dist_ancestor_scid)?;
        Ok(dist_ancestor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;

    use fbinit::FacebookInit;
    use futures::compat::Future01CompatExt;

    use fixtures::{linear, merge_even, merge_uneven};
    use mercurial_types::HgChangesetId;

    async fn validate_location_to_name(
        ctx: CoreContext,
        repo: BlobRepo,
        known: &'static str,
        distance: u64,
        expected: &'static str,
    ) -> Result<()> {
        let known_cs = repo
            .get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(known)?)
            .compat()
            .await?
            .unwrap();

        let mut dag = Dag::new();
        dag.build_up(&ctx, &repo, known_cs).await?;

        let answer = dag.location_to_name(known_cs, distance).await?;
        let expected_cs = repo
            .get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(expected)?)
            .compat()
            .await?
            .unwrap();
        assert_eq!(answer, expected_cs);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_location_to_name(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_location_to_name(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
            4,
            "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
        )
        .await?;
        validate_location_to_name(
            ctx.clone(),
            merge_even::getrepo(fb).await,
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
            2,
            "d7542c9db7f4c77dab4b315edd328edf1514952f",
        )
        .await?;
        validate_location_to_name(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "264f01429683b3dd8042cb3979e8bf37007118bc",
            5,
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
        )
        .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_two_build_up_calls(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;
        let mut dag = Dag::new();

        let known_cs = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0")?,
            )
            .compat()
            .await?
            .unwrap();
        dag.build_up(&ctx, &repo, known_cs).await?;
        let distance = 2;
        let answer = dag.location_to_name(known_cs, distance).await?;
        let expected_cs = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?,
            )
            .compat()
            .await?
            .unwrap();
        assert_eq!(answer, expected_cs);

        let known_cs = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("0ed509bf086fadcb8a8a5384dc3b550729b0fc17")?,
            )
            .compat()
            .await?
            .unwrap();
        dag.build_up(&ctx, &repo, known_cs).await?;
        let distance = 3;
        let answer = dag.location_to_name(known_cs, distance).await?;
        let expected_cs = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0")?,
            )
            .compat()
            .await?
            .unwrap();
        assert_eq!(answer, expected_cs);

        Ok(())
    }
}
