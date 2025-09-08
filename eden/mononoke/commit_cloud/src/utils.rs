/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use commit_cloud_types::ChangesetScheme;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;

use crate::ctx::CommitCloudContext;

pub async fn get_bonsai_from_cloud_ids(
    ctx: &CoreContext,
    cctx: &CommitCloudContext,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    cids: Vec<CloudChangesetId>,
) -> Result<Vec<(CloudChangesetId, ChangesetId)>> {
    match cctx.default_changeset_scheme {
        ChangesetScheme::Hg => {
            let hgids = cids
                .iter()
                .map(|cid| cid.clone().into())
                .collect::<Vec<HgChangesetId>>();

            bonsai_hg_mapping
                .get(ctx, hgids.into())
                .await?
                .into_iter()
                .map(|entry| Ok((entry.hg_cs_id.into(), entry.bcs_id)))
                .collect()
        }
        ChangesetScheme::Git => {
            let gitids = cids
                .iter()
                .map(|cid| GitSha1::from(cid.clone()))
                .collect::<Vec<GitSha1>>();

            bonsai_git_mapping
                .get(ctx, gitids.into())
                .await?
                .into_iter()
                .map(|entry| Ok((entry.git_sha1.into(), entry.bcs_id)))
                .collect()
        }
    }
}

pub async fn get_cloud_ids_from_bonsais(
    ctx: &CoreContext,
    cctx: &CommitCloudContext,
    cs_ids: Vec<ChangesetId>,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
) -> Result<HashMap<ChangesetId, CloudChangesetId>> {
    match cctx.default_changeset_scheme {
        ChangesetScheme::Hg => Ok(bonsai_hg_mapping
            .get(ctx, cs_ids.into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, CloudChangesetId::from(entry.hg_cs_id)))
            .collect::<HashMap<ChangesetId, CloudChangesetId>>()),
        ChangesetScheme::Git => Ok(bonsai_git_mapping
            .get(ctx, cs_ids.into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, CloudChangesetId::from(entry.git_sha1)))
            .collect::<HashMap<ChangesetId, CloudChangesetId>>()),
    }
}
