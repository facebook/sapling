/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use blobrepo::BlobRepoInner;
use blobstore::Blobstore;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use cacheblob::LeaseOps;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use filenodes::ArcFilenodes;
use repo_blobstore::RepoBlobstore;
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
        let repo_derived_data = Arc::new(
            self.repo_derived_data
                .with_replaced_lease(derived_data_lease),
        );
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
        let blobstore = RepoBlobstore::new_with_wrapped_inner_blobstore(
            self.repo_blobstore.as_ref().clone(),
            modify,
        );
        let repo_derived_data = Arc::new(
            self.repo_derived_data
                .with_replaced_blobstore(blobstore.clone()),
        );
        let repo_blobstore = Arc::new(blobstore);
        Self {
            repo_blobstore,
            repo_derived_data,
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
        let repo_derived_data = Arc::new(
            self.repo_derived_data
                .with_replaced_changesets(changesets.clone()),
        );

        Self {
            changesets,
            changeset_fetcher,
            repo_derived_data,
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
        let repo_derived_data = Arc::new(
            self.repo_derived_data
                .with_replaced_filenodes(filenodes.clone()),
        );
        Self {
            filenodes,
            repo_derived_data,
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
        let repo_derived_data = Arc::new(
            self.repo_derived_data
                .with_replaced_bonsai_hg_mapping(bonsai_hg_mapping.clone()),
        );
        Self {
            bonsai_hg_mapping,
            repo_derived_data,
            ..self.clone()
        }
    }
}
