/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use context::CoreContext;
use derived_data::{impl_bonsai_derived_mapping, BlobstoreRootIdMapping, BonsaiDerived};
use mononoke_types::BonsaiChangeset;

use crate::ChangesetInfo;

#[async_trait]
impl BonsaiDerived for ChangesetInfo {
    const NAME: &'static str = "changeset_info";
    type Mapping = ChangesetInfoMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        ChangesetInfoMapping::new(repo.blobstore().boxed())
    }

    async fn derive_from_parents(
        _ctx: CoreContext,
        _repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let csid = bonsai.get_changeset_id();
        Ok(ChangesetInfo::new(csid, bonsai))
    }
}

#[derive(Clone)]
pub struct ChangesetInfoMapping {
    blobstore: Arc<dyn Blobstore>,
}

impl ChangesetInfoMapping {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }
}

#[async_trait]
impl BlobstoreRootIdMapping for ChangesetInfoMapping {
    type Value = ChangesetInfo;

    fn blobstore(&self) -> &dyn Blobstore {
        &self.blobstore
    }

    fn prefix(&self) -> &'static str {
        "changeset_info.blake2."
    }
}

impl_bonsai_derived_mapping!(ChangesetInfoMapping, BlobstoreRootIdMapping);

#[cfg(test)]
mod test {
    use super::*;

    use blobrepo_hg::BlobRepoHg;
    use blobstore::Loadable;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use mercurial_types::HgChangesetId;
    use mononoke_types::BonsaiChangeset;
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn derive_info_test(fb: FacebookInit) {
        let mut runtime = Runtime::new().unwrap();
        let repo = runtime.block_on_std(linear::getrepo(fb));
        let ctx = CoreContext::test_mock(fb);

        let hg_cs_id = HgChangesetId::from_str("3c15267ebf11807f3d772eb891272b911ec68759").unwrap();
        let bcs_id = runtime
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();
        let bcs = runtime
            .block_on_std(bcs_id.load(&ctx, repo.blobstore()))
            .unwrap();

        // Make sure that the changeset info was saved in the blobstore
        let info = runtime
            .block_on_std(ChangesetInfo::derive(&ctx, &repo, bcs_id))
            .unwrap();

        check_info(&info, &bcs);
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
}
