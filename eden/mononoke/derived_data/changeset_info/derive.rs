/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::ChangesetInfo;

use derived_data_service_if::types as thrift;

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "changeset_info.blake2.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<ChangesetInfo>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for ChangesetInfo {
    const NAME: &'static str = "changeset_info";

    type Dependencies = dependencies![];

    async fn derive_single(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        Ok(ChangesetInfo::new(bonsai.get_changeset_id(), bonsai))
    }

    async fn derive_batch(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        // Derivation with gaps doesn't make much sense for changeset info, so
        // ignore the gap size.
        Ok(bonsais
            .into_iter()
            .map(|bonsai| {
                let csid = bonsai.get_changeset_id();
                (csid, ChangesetInfo::new(csid, bonsai))
            })
            .collect())
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::changeset_info(
            thrift::DerivedDataChangesetInfo::changeset_info(data),
        ) = data
        {
            Self::from_thrift(data)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::changeset_info(
            thrift::DerivedDataChangesetInfo::changeset_info(data.into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(ChangesetInfo);

#[cfg(test)]
mod test {
    use super::*;

    use blobstore::Loadable;
    use derived_data_manager::BatchDeriveOptions;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::compat::Stream01CompatExt;
    use futures::TryStreamExt;
    use mercurial_types::HgChangesetId;
    use mononoke_types::BonsaiChangeset;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tests_utils::resolve_cs_id;

    #[fbinit::test]
    async fn derive_info_test(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let manager = repo.repo_derived_data().manager();

        let hg_cs_id = HgChangesetId::from_str("3c15267ebf11807f3d772eb891272b911ec68759").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await?
            .unwrap();
        let bcs = bcs_id.load(&ctx, repo.blobstore()).await?;
        // Make sure that the changeset info was saved in the blobstore
        let info = manager.derive(&ctx, bcs_id, None).await?;

        check_info(&info, &bcs);
        Ok(())
    }

    fn check_info(info: &ChangesetInfo, bcs: &BonsaiChangeset) {
        assert_eq!(*info.changeset_id(), bcs.get_changeset_id());
        assert_eq!(info.message(), bcs.message());
        assert_eq!(
            info.parents().collect::<Vec<_>>(),
            bcs.parents().collect::<Vec<_>>()
        );
        assert_eq!(info.author(), bcs.author());
        assert_eq!(info.author_date(), bcs.author_date());
        assert_eq!(info.committer(), bcs.committer());
        assert_eq!(info.committer_date(), bcs.committer_date());
        assert_eq!(
            info.extra().collect::<BTreeMap<_, _>>(),
            bcs.extra().collect::<BTreeMap<_, _>>()
        );
    }

    #[fbinit::test]
    async fn batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let manager = repo.repo_derived_data().manager();

        let mut cs_ids =
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                .compat()
                .try_collect::<Vec<_>>()
                .await?;
        cs_ids.reverse();
        manager
            .backfill_batch::<ChangesetInfo>(
                &ctx,
                cs_ids.clone(),
                BatchDeriveOptions::Parallel { gap_size: None },
                None,
            )
            .await?;
        let cs_infos = manager
            .fetch_derived_batch(&ctx, cs_ids.clone(), None)
            .await?;

        for cs_id in cs_ids {
            let bonsai = cs_id.load(&ctx, repo.blobstore()).await?;
            let cs_info = cs_infos
                .get(&cs_id)
                .expect("ChangesetInfo should have been derived");
            check_info(cs_info, &bonsai);
        }

        Ok(())
    }
}
