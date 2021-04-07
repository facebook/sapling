/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use blobrepo::{BlobRepo, BlobRepoInner};
use blobstore::Blobstore;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use cacheblob::LeaseOps;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use filenodes::ArcFilenodes;
use repo_blobstore::RepoBlobstoreArgs;
use repo_derived_data::RepoDerivedData;
use std::sync::Arc;

/// Create new instance of implementing object with overridden field of specified type.
///
/// This override can be very dangerous, it should only be used in unittest, or if you
/// really know what you are doing.
pub trait DangerousOverride<T> {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(T) -> T;
}

impl<T> DangerousOverride<T> for BlobRepo
where
    BlobRepoInner: DangerousOverride<T>,
{
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(T) -> T,
    {
        let inner = (*self.inner()).clone().dangerous_override(modify);
        BlobRepo::from_inner_dangerous(inner)
    }
}

impl DangerousOverride<Arc<dyn LeaseOps>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn LeaseOps>) -> Arc<dyn LeaseOps>,
    {
        let derived_data_lease = modify(self.repo_derived_data.lease().clone());
        let repo_derived_data = Arc::new(RepoDerivedData::new(
            self.repo_derived_data.config().clone(),
            derived_data_lease,
        ));
        Self {
            repo_derived_data,
            ..self.clone()
        }
    }
}

impl DangerousOverride<Arc<dyn Blobstore>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn Blobstore>) -> Arc<dyn Blobstore>,
    {
        let (blobstore, repoid) = RepoBlobstoreArgs::new_with_wrapped_inner_blobstore(
            self.repo_blobstore.as_ref().clone(),
            self.repoid,
            modify,
        )
        .into_blobrepo_parts();
        let repo_blobstore = Arc::new(blobstore);
        Self {
            repoid,
            repo_blobstore,
            ..self.clone()
        }
    }
}

impl DangerousOverride<ArcChangesets> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(ArcChangesets) -> ArcChangesets,
    {
        let changesets = modify(self.changesets.clone());
        let changeset_fetcher =
            Arc::new(SimpleChangesetFetcher::new(changesets.clone(), self.repoid));

        Self {
            changesets,
            changeset_fetcher,
            ..self.clone()
        }
    }
}

impl DangerousOverride<ArcFilenodes> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(ArcFilenodes) -> ArcFilenodes,
    {
        let filenodes = modify(self.filenodes.clone());
        Self {
            filenodes,
            ..self.clone()
        }
    }
}

impl DangerousOverride<ArcBonsaiHgMapping> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(ArcBonsaiHgMapping) -> ArcBonsaiHgMapping,
    {
        let bonsai_hg_mapping = modify(self.bonsai_hg_mapping.clone());
        Self {
            bonsai_hg_mapping,
            ..self.clone()
        }
    }
}
