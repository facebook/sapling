/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use cacheblob::LeaseOps;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use filestore::FilestoreConfig;
use git_symbolic_refs::GitSymbolicRefs;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

#[facet::container]
#[derive(Clone)]
pub(crate) struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    git_symbolic_refs: dyn GitSymbolicRefs,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
}

impl DangerousOverride<Arc<dyn Blobstore>> for Repo {
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

impl DangerousOverride<ArcBonsaiHgMapping> for Repo {
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

impl DangerousOverride<Arc<dyn LeaseOps>> for Repo {
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
