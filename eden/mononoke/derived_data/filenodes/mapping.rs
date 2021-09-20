/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::{Blobstore, Loadable};
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingArc};
use context::CoreContext;
use derived_data::{
    BonsaiDerivable, BonsaiDerivedMapping, BonsaiDerivedMappingContainer, BonsaiDerivedOld,
    DeriveError, DerivedDataTypesConfig,
};
use filenodes::{FilenodeInfo, FilenodeResult, Filenodes, FilenodesArc, PreparedFilenode};
use futures::{stream, StreamExt, TryFutureExt, TryStreamExt};
use mercurial_types::{HgChangesetId, HgFileNodeId, NULL_HASH};
use mononoke_types::{BonsaiChangeset, ChangesetId, RepoPath, RepositoryId};
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use std::sync::Arc;
use std::{collections::HashMap, convert::TryFrom};

use crate::derive::{derive_filenodes, derive_filenodes_in_batch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRootFilenode {
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl From<PreparedRootFilenode> for PreparedFilenode {
    fn from(root_filenode: PreparedRootFilenode) -> Self {
        let PreparedRootFilenode {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = root_filenode;

        PreparedFilenode {
            path: RepoPath::RootPath,
            info: FilenodeInfo {
                filenode,
                p1,
                p2,
                copyfrom,
                linknode,
            },
        }
    }
}

impl TryFrom<PreparedFilenode> for PreparedRootFilenode {
    type Error = Error;

    fn try_from(filenode: PreparedFilenode) -> Result<Self, Self::Error> {
        let PreparedFilenode { path, info } = filenode;

        let FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = info;

        if path != RepoPath::RootPath {
            return Err(format_err!("unexpected path for root filenode: {:?}", path));
        }
        Ok(Self {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    }
}

/// Derives filenodes that are stores in Filenodes object (usually in a database).
/// Note: that should be derived only for public commits!
///
/// Filenodes might be disabled, in that case FilenodesOnlyPublic will always return
/// FilenodesOnlyPublic::Disabled enum variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilenodesOnlyPublic {
    Present {
        root_filenode: Option<PreparedRootFilenode>,
    },
    Disabled,
}

#[async_trait]
impl BonsaiDerivable for FilenodesOnlyPublic {
    const NAME: &'static str = "filenodes";

    type Options = ();

    async fn derive_from_parents_impl(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        _options: &Self::Options,
    ) -> Result<Self, Error> {
        derive_filenodes(&ctx, &repo, bonsai).await
    }

    async fn batch_derive_impl(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Vec<ChangesetId>,
        mapping: &BonsaiDerivedMappingContainer<Self>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>, Error> {
        let filenodes = repo.get_filenodes();
        let blobstore = repo.blobstore();
        let bonsais = stream::iter(
            csids
                .into_iter()
                .map(|bcs_id| async move { bcs_id.load(ctx, blobstore).await }),
        )
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;
        let prepared = derive_filenodes_in_batch(ctx, repo, bonsais).await?;
        let mut res = HashMap::with_capacity(prepared.len());
        for (cs_id, public_filenode, non_roots) in prepared.into_iter() {
            let filenode = match public_filenode {
                FilenodesOnlyPublic::Present { root_filenode } => match root_filenode {
                    Some(filenode) if !non_roots.is_empty() => {
                        match filenodes.add_filenodes(ctx, non_roots).await? {
                            FilenodeResult::Disabled => FilenodesOnlyPublic::Disabled,
                            FilenodeResult::Present(()) => FilenodesOnlyPublic::Present {
                                root_filenode: Some(filenode),
                            },
                        }
                    }
                    _ => FilenodesOnlyPublic::Present { root_filenode },
                },
                FilenodesOnlyPublic::Disabled => FilenodesOnlyPublic::Disabled,
            };
            res.insert(cs_id, filenode.clone());
            if let FilenodesOnlyPublic::Disabled = filenode {
                continue;
            }
            mapping.put(ctx, cs_id, &filenode).await?;
        }
        Ok(res)
    }
}

#[derive(Clone)]
pub struct FilenodesOnlyPublicMapping {
    repo_id: RepositoryId,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    filenodes: Arc<dyn Filenodes>,
    blobstore: Arc<dyn Blobstore>,
}

impl FilenodesOnlyPublicMapping {
    pub fn new(
        repo: &(impl RepoIdentityRef + BonsaiHgMappingArc + FilenodesArc + RepoBlobstoreRef),
        _config: &DerivedDataTypesConfig,
    ) -> Result<Self, DeriveError> {
        Ok(Self {
            repo_id: repo.repo_identity().id(),
            bonsai_hg_mapping: repo.bonsai_hg_mapping_arc(),
            filenodes: repo.filenodes_arc(),
            blobstore: repo.repo_blobstore().boxed(),
        })
    }
}

#[async_trait]
impl BonsaiDerivedOld for FilenodesOnlyPublic {
    type DefaultMapping = FilenodesOnlyPublicMapping;

    fn default_mapping(
        _ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<Self::DefaultMapping, DeriveError> {
        let config = derived_data::enabled_type_config(repo, Self::NAME)?;
        FilenodesOnlyPublicMapping::new(repo, config)
    }
}

#[async_trait]
impl BonsaiDerivedMapping for FilenodesOnlyPublicMapping {
    type Value = FilenodesOnlyPublic;

    async fn get(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        stream::iter(csids.into_iter())
            .map({
                move |cs_id| async move {
                    let filenode_res = self.fetch_root_filenode(ctx, cs_id).await?;
                    let maybe_root_filenode = match filenode_res {
                        FilenodeResult::Present(maybe_root_filenode) => maybe_root_filenode,
                        FilenodeResult::Disabled => {
                            return Ok(Some((cs_id, FilenodesOnlyPublic::Disabled)));
                        }
                    };

                    Ok(maybe_root_filenode.map(move |filenode| {
                        (
                            cs_id,
                            FilenodesOnlyPublic::Present {
                                root_filenode: Some(filenode),
                            },
                        )
                    }))
                }
            })
            .buffer_unordered(100)
            .try_filter_map(|x| async { Ok(x) })
            .try_collect()
            .await
    }

    async fn put(
        &self,
        ctx: &CoreContext,
        _csid: ChangesetId,
        id: &Self::Value,
    ) -> Result<(), Error> {
        let root_filenode = match id {
            FilenodesOnlyPublic::Present { root_filenode } => root_filenode.as_ref(),
            FilenodesOnlyPublic::Disabled => None,
        };

        match root_filenode {
            Some(root_filenode) => {
                self.filenodes
                    .add_filenodes(ctx, vec![root_filenode.clone().into()])
                    .map_ok(|res| match res {
                        // If filenodes are disabled then just return success
                        // but use explicit match here in case we add more variants
                        // to FilenodeResult enum
                        FilenodeResult::Present(()) | FilenodeResult::Disabled => {}
                    })
                    .await
            }
            None => Ok(()),
        }
    }

    fn options(&self) {}
}

impl FilenodesOnlyPublicMapping {
    async fn fetch_root_filenode(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<FilenodeResult<Option<PreparedRootFilenode>>, Error> {
        // If hg changeset is not generated, then root filenode can't possible be generated
        // Check it and return None if hg changeset is not generated
        let maybe_hg_cs_id = self
            .bonsai_hg_mapping
            .get_hg_from_bonsai(ctx, self.repo_id, cs_id)
            .await?;
        let hg_cs_id = if let Some(hg_cs_id) = maybe_hg_cs_id {
            hg_cs_id
        } else {
            return Ok(FilenodeResult::Present(None));
        };

        let mf_id = hg_cs_id.load(ctx, &self.blobstore).await?.manifestid();

        // Special case null manifest id if we run into it
        let mf_id = mf_id.into_nodehash();
        if mf_id == NULL_HASH {
            Ok(FilenodeResult::Present(Some(PreparedRootFilenode {
                filenode: HgFileNodeId::new(NULL_HASH),
                p1: None,
                p2: None,
                copyfrom: None,
                linknode: HgChangesetId::new(NULL_HASH),
            })))
        } else {
            let filenode_res = self
                .filenodes
                .get_filenode(ctx, &RepoPath::RootPath, HgFileNodeId::new(mf_id))
                .await?;

            match filenode_res {
                FilenodeResult::Present(maybe_info) => {
                    let info = maybe_info
                        .map(|info| {
                            PreparedRootFilenode::try_from(PreparedFilenode {
                                path: RepoPath::RootPath,
                                info,
                            })
                        })
                        .transpose()?;
                    Ok(FilenodeResult::Present(info))
                }
                FilenodeResult::Disabled => Ok(FilenodeResult::Disabled),
            }
        }
    }
}
