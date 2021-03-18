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
use blobstore::Loadable;
use context::CoreContext;
use derived_data::{
    BonsaiDerivable, BonsaiDerived, BonsaiDerivedMapping, DeriveError, DerivedDataTypesConfig,
};
use filenodes::{FilenodeInfo, FilenodeResult, PreparedFilenode};
use futures::{compat::Future01CompatExt, stream, StreamExt, TryFutureExt, TryStreamExt};
use itertools::{Either, Itertools};
use mercurial_types::{HgChangesetId, HgFileNodeId, NULL_HASH};
use mononoke_types::{BonsaiChangeset, ChangesetId, RepoPath};
use std::{collections::HashMap, convert::TryFrom};

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
        let filenodes =
            crate::derive::generate_all_filenodes(&ctx, &repo, bonsai.get_changeset_id()).await?;

        if filenodes.is_empty() {
            // This commit didn't create any new filenodes, and it's root manifest is the
            // same as one of the parents (that can happen if this commit is empty).
            // In that case we don't need to insert a root filenode - it will be inserted
            // when parent is derived.
            Ok(FilenodesOnlyPublic::Present {
                root_filenode: None,
            })
        } else {
            let (roots, non_roots): (Vec<_>, Vec<_>) =
                filenodes.into_iter().partition_map(classify_filenode);
            let mut roots = roots.into_iter();

            match (roots.next(), roots.next()) {
                (Some(root_filenode), None) => {
                    let filenodes = repo.get_filenodes();
                    let repo_id = repo.get_repoid();
                    let filenode_res = filenodes
                        .add_filenodes(ctx.clone(), non_roots, repo_id)
                        .compat()
                        .await?;

                    match filenode_res {
                        FilenodeResult::Present(()) => Ok(FilenodesOnlyPublic::Present {
                            root_filenode: Some(root_filenode),
                        }),
                        FilenodeResult::Disabled => Ok(FilenodesOnlyPublic::Disabled),
                    }
                }
                _ => Err(format_err!("expected exactly one root, found {:?}", roots)),
            }
        }
    }
}

fn classify_filenode(filenode: PreparedFilenode) -> Either<PreparedRootFilenode, PreparedFilenode> {
    if filenode.path == RepoPath::RootPath {
        let FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = filenode.info;

        Either::Left(PreparedRootFilenode {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    } else {
        Either::Right(filenode)
    }
}

#[derive(Clone)]
pub struct FilenodesOnlyPublicMapping {
    repo: BlobRepo,
}

impl FilenodesOnlyPublicMapping {
    pub fn new(repo: &BlobRepo, _config: &DerivedDataTypesConfig) -> Result<Self, DeriveError> {
        Ok(Self { repo: repo.clone() })
    }
}

#[async_trait]
impl BonsaiDerived for FilenodesOnlyPublic {
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
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        stream::iter(csids.into_iter())
            .map({
                let repo = &self.repo;
                let ctx = &ctx;
                move |cs_id| async move {
                    let filenode_res = fetch_root_filenode(&ctx, cs_id, &repo).await?;
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
        ctx: CoreContext,
        _csid: ChangesetId,
        id: Self::Value,
    ) -> Result<(), Error> {
        let filenodes = self.repo.get_filenodes();
        let repo_id = self.repo.get_repoid();

        let root_filenode = match id {
            FilenodesOnlyPublic::Present { root_filenode } => root_filenode,
            FilenodesOnlyPublic::Disabled => None,
        };

        match root_filenode {
            Some(root_filenode) => {
                filenodes
                    .add_filenodes(ctx.clone(), vec![root_filenode.into()], repo_id)
                    .compat()
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

async fn fetch_root_filenode(
    ctx: &CoreContext,
    cs_id: ChangesetId,
    repo: &BlobRepo,
) -> Result<FilenodeResult<Option<PreparedRootFilenode>>, Error> {
    // If hg changeset is not generated, then root filenode can't possible be generated
    // Check it and return None if hg changeset is not generated
    let maybe_hg_cs_id = repo
        .get_bonsai_hg_mapping()
        .get_hg_from_bonsai(ctx, repo.get_repoid(), cs_id.clone())
        .await?;
    let hg_cs_id = if let Some(hg_cs_id) = maybe_hg_cs_id {
        hg_cs_id
    } else {
        return Ok(FilenodeResult::Present(None));
    };

    let mf_id = hg_cs_id.load(ctx, repo.blobstore()).await?.manifestid();

    // Special case null manifest id if we run into it
    let mf_id = mf_id.into_nodehash();
    let filenodes = repo.get_filenodes();
    if mf_id == NULL_HASH {
        Ok(FilenodeResult::Present(Some(PreparedRootFilenode {
            filenode: HgFileNodeId::new(NULL_HASH),
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: HgChangesetId::new(NULL_HASH),
        })))
    } else {
        let filenode_res = filenodes
            .get_filenode(
                ctx.clone(),
                &RepoPath::RootPath,
                HgFileNodeId::new(mf_id),
                repo.get_repoid(),
            )
            .compat()
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
