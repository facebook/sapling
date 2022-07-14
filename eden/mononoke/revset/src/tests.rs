/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BonsaiNodeStream;
use anyhow::format_err;
use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures_ext::StreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use revset_test_helper::single_changeset_id;
use revset_test_helper::string_to_bonsai;
use std::any::Any;
use std::collections::HashMap;

pub struct TestChangesetFetcher {
    repo: BlobRepo,
}

impl TestChangesetFetcher {
    pub fn new(repo: BlobRepo) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ChangesetFetcher for TestChangesetFetcher {
    async fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation, Error> {
        let genopt = self.repo.get_generation_number(ctx, cs_id).await?;
        let gen = genopt.ok_or_else(|| format_err!("{} not found", cs_id))?;
        Ok(gen)
    }

    async fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        self.repo.get_changeset_parents_by_bonsai(ctx, cs_id).await
    }

    fn get_stats(&self) -> HashMap<String, Box<dyn Any>> {
        HashMap::new()
    }
}

pub async fn get_single_bonsai_streams(
    ctx: CoreContext,
    repo: &BlobRepo,
    hashes: &[&str],
) -> Vec<BonsaiNodeStream> {
    let mut ret = vec![];

    for hash in hashes {
        let stream = single_changeset_id(
            ctx.clone(),
            string_to_bonsai(ctx.fb, &repo.clone(), hash).await,
            repo,
        )
        .boxify();
        ret.push(stream)
    }

    ret
}
