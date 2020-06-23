/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use blobrepo::{BlobRepo, BlobRepoInner};
use blobstore::Blobstore;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use cacheblob::LeaseOps;
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::Changesets;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use metaconfig_types::DerivedDataConfig;
use repo_blobstore::RepoBlobstoreArgs;
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
        let derived_data_lease = modify(self.derived_data_lease.clone());
        Self {
            derived_data_lease,
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
            self.blobstore.clone(),
            self.repoid,
            modify,
        )
        .into_blobrepo_parts();
        Self {
            repoid,
            blobstore,
            ..self.clone()
        }
    }
}

impl DangerousOverride<Arc<dyn Bookmarks>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn Bookmarks>) -> Arc<dyn Bookmarks>,
    {
        let bookmarks = modify(self.bookmarks.clone());
        Self {
            bookmarks,
            ..self.clone()
        }
    }
}

impl DangerousOverride<Arc<dyn Changesets>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn Changesets>) -> Arc<dyn Changesets>,
    {
        let changesets = modify(self.changesets.clone());
        let changeset_fetcher_factory = {
            let repoid = self.repoid;
            let changesets = changesets.clone();
            move || {
                let res: Arc<dyn ChangesetFetcher + Send + Sync> = Arc::new(
                    SimpleChangesetFetcher::new(changesets.clone(), repoid.clone()),
                );
                res
            }
        };

        Self {
            changesets,
            changeset_fetcher_factory: Arc::new(changeset_fetcher_factory),
            ..self.clone()
        }
    }
}

impl DangerousOverride<Arc<dyn Filenodes>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn Filenodes>) -> Arc<dyn Filenodes>,
    {
        let filenodes = match self.attributes.get::<dyn Filenodes>() {
            Some(attr) => modify(attr.clone()),
            None => panic!("BlboRepo initalized incorrectly and does not have Filenodes attribute"),
        };
        let mut attrs = self.attributes.as_ref().clone();
        attrs.insert::<dyn Filenodes>(filenodes);
        Self {
            attributes: Arc::new(attrs),
            ..self.clone()
        }
    }
}

impl DangerousOverride<Arc<dyn BonsaiHgMapping>> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn BonsaiHgMapping>) -> Arc<dyn BonsaiHgMapping>,
    {
        let bonsai_hg_mapping = match self.attributes.get::<dyn BonsaiHgMapping>() {
            Some(attr) => modify(attr.clone()),
            None => panic!(
                "BlboRepo initalized incorrectly and does not have BonsaiHgMapping attribute",
            ),
        };
        let mut attrs = self.attributes.as_ref().clone();
        attrs.insert::<dyn BonsaiHgMapping>(bonsai_hg_mapping);
        Self {
            attributes: Arc::new(attrs),
            ..self.clone()
        }
    }
}

impl DangerousOverride<DerivedDataConfig> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(DerivedDataConfig) -> DerivedDataConfig,
    {
        let derived_data_config = modify(self.derived_data_config.clone());
        Self {
            derived_data_config,
            ..self.clone()
        }
    }
}

impl DangerousOverride<FilestoreConfig> for BlobRepoInner {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(FilestoreConfig) -> FilestoreConfig,
    {
        let filestore_config = modify(self.filestore_config.clone());
        Self {
            filestore_config,
            ..self.clone()
        }
    }
}
